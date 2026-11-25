use crate::{
    backend::*,
    controller::ConveyorController,
    domain::*,
    input::HardwareInputService,
    persistence::LocalEventStore,
    ports::{Persistence, SerialConnector, SerialTransport},
    protocol::{Frame, SerialProtocolCodec},
    serial::UsbSerialConnector,
    simulator::{self, SimControl},
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiAction {
    Connect { port: String },
    Disconnect {},
    Simulation { enabled: bool },
    Simulate { control: SimControl },
    ManualMove { station: u8, confirmed: bool },
    Stop {},
    ResetFault { confirmed: bool },
    Inspect { confirmed: bool },
    Pause { paused: bool },
    ReconnectBackend {},
    Enroll { code: String },
    SaveSettings { settings: Settings },
    Shutdown {},
}
type Reply = oneshot::Sender<Result<()>>;
enum Message {
    Request(UiAction, Reply),
    Remote(RemoteCommand),
    Acked(RemoteCommand, bool),
    Online(DeviceConfig),
    Identity(Option<Uuid>),
    Offline,
    ButtonDropped,
    Log(&'static str),
    NetworkFailed,
}
enum NetJob {
    Ack(RemoteCommand),
    Button(HardwareEvent),
    Enroll(String, Reply),
    Reconnect,
}
#[derive(Clone)]
pub struct GatewayHandle {
    tx: mpsc::Sender<Message>,
    pub status: watch::Receiver<GatewayStatus>,
}
impl GatewayHandle {
    pub fn snapshot(&self) -> GatewayStatus {
        self.status.borrow().clone()
    }
    pub async fn execute(&self, action: UiAction) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .try_send(Message::Request(action, tx))
            .map_err(|_| Error::Unavailable)?;
        tokio::time::timeout(Duration::from_secs(35), rx)
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?
    }
    pub fn diagnostics(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.snapshot()).map_err(|_| Error::Config("diagnostics"))
    }
}
pub struct GatewaySupervisor;
impl GatewaySupervisor {
    pub fn start(
        path: PathBuf,
        vault: Arc<dyn CredentialStore>,
    ) -> Result<(GatewayHandle, tokio::task::JoinHandle<Result<()>>)> {
        Self::start_with_connector(path, vault, Arc::new(UsbSerialConnector))
    }
    pub fn start_with_connector(
        path: PathBuf,
        vault: Arc<dyn CredentialStore>,
        connector: Arc<dyn SerialConnector>,
    ) -> Result<(GatewayHandle, tokio::task::JoinHandle<Result<()>>)> {
        let mut store = LocalEventStore::open(&path)?;
        store.recover()?;
        let history = store.history()?;
        let settings = store.settings()?;
        settings.validate()?;
        // Release cannot silently permit plaintext, including settings written by a debug build.
        BackendApiClient::new(
            &settings.backend_url,
            cfg!(debug_assertions) && settings.allow_loopback_http,
        )?;
        let mut controller = ConveyorController::default();
        if store.inspection()? {
            controller.inspection = true;
            controller.state = ConveyorState::NeedsInspection;
        }
        let status = GatewayStatus {
            settings,
            history,
            state: controller.state,
            ..GatewayStatus::default()
        };
        let (status_tx, status_rx) = watch::channel(status.clone());
        let (tx, rx) = mpsc::channel(128);
        let (net_tx, net_rx) = mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = GatewayHandle {
            tx: tx.clone(),
            status: status_rx.clone(),
        };
        let task = tokio::spawn(async move {
            let mut network = tokio::spawn(async move {
                let result =
                    network_worker(path, vault, status_rx, tx.clone(), net_rx, shutdown_rx).await;
                if result.is_err() {
                    notify(&tx, Message::NetworkFailed).await;
                }
                result
            });
            let mut actor = HardwareActor {
                status,
                controller,
                input: HardwareInputService::default(),
                store,
                serial: None,
                connector,
                sim: None,
                sim_task: None,
                codec: SerialProtocolCodec::default(),
                tx: status_tx,
                net: net_tx,
                started: Instant::now(),
                last_rx: Instant::now(),
                last_ping: Instant::now(),
                last_status: Instant::now(),
                handshake_deadline: Instant::now(),
                nonce: String::new(),
                pending: None,
                reconnect_at: Instant::now(),
                retry: 0,
                wants_connection: false,
                binding_expiry: None,
                fresh_status: None,
            };
            let result = actor.run(rx).await;
            if let Err(e) = &result {
                actor.status.paused = true;
                actor.status.backend_connected = false;
                actor.controller.uncertain("SUPERVISOR_STOPPED");
                actor.log("error", e.to_string());
                actor.publish();
            }
            // Fail closed even if storage/actor fails: serial is dropped; firmware watchdog stops motor.
            drop(actor);
            let _receivers = shutdown_tx.send(true);
            match tokio::time::timeout(Duration::from_secs(3), &mut network).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => tracing::error!(error=%e,"backend worker stopped"),
                Ok(Err(e)) => tracing::error!(error=%e,"backend worker join failed"),
                Err(_) => network.abort(),
            }
            result
        });
        Ok((handle, task))
    }
}
struct HardwareActor {
    status: GatewayStatus,
    controller: ConveyorController,
    input: HardwareInputService,
    store: LocalEventStore,
    serial: Option<Box<dyn SerialTransport>>,
    connector: Arc<dyn SerialConnector>,
    sim: Option<mpsc::Sender<SimControl>>,
    sim_task: Option<tokio::task::JoinHandle<Result<()>>>,
    codec: SerialProtocolCodec,
    tx: watch::Sender<GatewayStatus>,
    net: mpsc::Sender<NetJob>,
    started: Instant,
    last_rx: Instant,
    last_ping: Instant,
    last_status: Instant,
    handshake_deadline: Instant,
    nonce: String,
    pending: Option<(RemoteCommand, Uuid)>,
    reconnect_at: Instant,
    retry: u32,
    wants_connection: bool,
    binding_expiry: Option<DateTime<Utc>>,
    fresh_status: Option<Instant>,
}
impl Drop for HardwareActor {
    fn drop(&mut self) {
        if let Some(task) = self.sim_task.take() {
            task.abort();
        }
    }
}
impl HardwareActor {
    fn log(&mut self, level: &str, message: impl Into<String>) {
        let message = message.into();
        tracing::info!(level, message, "gateway event");
        self.status.logs.insert(
            0,
            LogEntry {
                at: Utc::now(),
                level: level.into(),
                message,
            },
        );
        self.status.logs.truncate(200);
    }
    fn publish(&mut self) {
        self.status.state = self.controller.state;
        self.status.safe = self.controller.safe
            && !self.controller.inspection
            && self
                .fresh_status
                .is_some_and(|observed| observed.elapsed() < Duration::from_secs(6));
        self.status.position = self.controller.position;
        self.status.active_command = self.controller.active.as_ref().map(|a| a.id);
        self.status.fault = self.controller.fault.clone();
        self.tx.send_if_modified(|previous| {
            if *previous == self.status {
                false
            } else {
                *previous = self.status.clone();
                true
            }
        });
    }
    fn record(&mut self, event: CommandExecutionResult) -> Result<()> {
        if self.controller.inspection {
            self.store.put("inspection", "true")?;
        }
        self.store.record(&event, self.status.simulation)?;
        self.status.history.insert(0, event);
        self.status.history.truncate(100);
        if self.controller.inspection {
            self.store.put("inspection", "true")?;
        }
        Ok(())
    }
    async fn write(&mut self, frame: Frame) -> Result<()> {
        let bytes = frame.encode()?;
        let serial = self.serial.as_mut().ok_or(Error::Unavailable)?;
        tokio::time::timeout(Duration::from_millis(300), serial.write(&bytes))
            .await
            .map_err(|_| Error::Unavailable)?
    }
    async fn connect(&mut self) -> Result<()> {
        if self.serial.is_some() {
            return Err(Error::Safety("disconnect first"));
        }
        if self.status.simulation {
            let (serial, control, task) = simulator::connect();
            self.serial = Some(Box::new(serial));
            self.sim = Some(control);
            self.sim_task = Some(task);
        } else {
            let port = self
                .status
                .settings
                .port
                .clone()
                .ok_or(Error::Config("select serial port"))?;
            self.serial = Some(
                self.connector
                    .open(&port, self.status.settings.baud)
                    .await?,
            );
        }
        self.status.session_id = Uuid::new_v4();
        self.input.reset(self.status.session_id);
        self.nonce = Uuid::new_v4().simple().to_string();
        self.codec = SerialProtocolCodec::default();
        self.status.usb_connected = true;
        self.status.handshake_complete = false;
        self.status.firmware = None;
        self.status.capabilities.clear();
        self.controller.connected();
        self.fresh_status = None;
        self.handshake_deadline = Instant::now() + Duration::from_secs(4);
        self.last_rx = Instant::now();
        self.last_ping = Instant::now();
        self.write(Frame::Hello {
            v: 1,
            nonce: self.nonce.clone(),
        })
        .await?;
        self.log("info", "Serial подключён; ожидается HELLO_ACK");
        Ok(())
    }
    async fn disconnect(&mut self) -> Result<()> {
        if self.controller.active.is_some() && self.serial.is_some() {
            if let Err(e) = self.write(Frame::Stop { v: 1 }).await {
                self.log("warn", e.to_string());
            }
        }
        if let Some(result) = self.controller.disconnected() {
            self.record(result)?;
        }
        if let Some((cmd, _)) = self.pending.take() {
            self.record(CommandExecutionResult::new(cmd.command_id, "REJECTED"))?;
        }
        self.serial = None;
        self.sim = None;
        if let Some(task) = self.sim_task.take() {
            task.abort();
        }
        self.status.usb_connected = false;
        self.status.handshake_complete = false;
        self.fresh_status = None;
        self.status.statistics.reconnects += 1;
        self.reconnect_at = Instant::now() + backoff(self.retry);
        self.retry = self.retry.saturating_add(1);
        self.log(
            "warn",
            "Serial отключён; старые команды и кнопки не повторяются",
        );
        Ok(())
    }
    async fn frame(&mut self, frame: Frame) -> Result<()> {
        self.status.statistics.frames += 1;
        if let Frame::HelloAck { nonce, fw, cap, .. } = &frame {
            if *nonce != self.nonce
                || !crate::protocol::supported_firmware(fw)
                || !cap.iter().any(|c| c == "BUTTONS")
                || !cap.iter().any(|c| c == "CONVEYOR")
            {
                self.log(
                    "error",
                    "Неверный nonce, возможности или неподдерживаемая прошивка",
                );
                self.wants_connection = false;
                return self.disconnect().await;
            }
            if self.status.handshake_complete {
                self.log("error", "Повторный HELLO_ACK: возможен reset");
                return self.disconnect().await;
            }
            self.status.handshake_complete = true;
            self.status.firmware = Some(fw.clone());
            self.status.capabilities = cap.clone();
            self.retry = 0;
            self.write(Frame::Status { v: 1 }).await?;
            self.log("info", "Handshake завершён; запрошен физический STATUS");
        } else if !self.status.handshake_complete {
            return Ok(());
        }
        self.last_rx = Instant::now();
        if let Frame::Button { seq, b, edge, .. } = frame {
            let event = self.input.accept(
                self.status.device_id.unwrap_or(Uuid::nil()),
                self.status.session_id,
                seq,
                b,
                edge,
                Utc::now(),
                self.status.simulation,
            );
            if let Some(mut event) = event {
                self.status.statistics.button_events += 1;
                if self
                    .binding_expiry
                    .is_some_and(|expiry| expiry > Utc::now())
                {
                    event.browser_session_id = self.status.browser_session_id;
                }
                if !event.simulation
                    && self.status.backend_connected
                    && event.browser_session_id.is_some()
                {
                    if self.net.try_send(NetJob::Button(event.clone())).is_err() {
                        self.status.statistics.dropped += 1;
                    }
                } else if !event.simulation {
                    self.status.statistics.dropped += 1;
                }
                self.status.buttons.insert(0, event);
                self.status.buttons.truncate(24);
            } else {
                self.status.statistics.duplicates += 1;
            }
        } else {
            if matches!(frame, Frame::State { .. }) {
                self.fresh_status = Some(Instant::now());
            }
            if let Some(result) = self.controller.frame(&frame) {
                self.record(result)?;
            }
            if self.controller.inspection {
                self.store.put("inspection", "true")?;
            }
        }
        Ok(())
    }
    async fn remote(&mut self, command: RemoteCommand) -> Result<()> {
        command.validate(
            self.status.device_id.ok_or(Error::Safety("not enrolled"))?,
            Utc::now(),
        )?;
        if !self.store.receive(command.command_id, false)? {
            self.log("warn", "Повтор команды отклонён журналом идемпотентности");
            return Ok(());
        }
        self.record(CommandExecutionResult::new(command.command_id, "RECEIVED"))?;
        if self.status.simulation
            || self.status.paused
            || !self.status.backend_connected
            || !self.controller.ready()
            || self.pending.is_some()
        {
            self.record(CommandExecutionResult::new(command.command_id, "REJECTED"))?;
            return Ok(());
        }
        self.pending = Some((command.clone(), self.status.session_id));
        if self.net.try_send(NetJob::Ack(command.clone())).is_err() {
            self.pending = None;
            self.record(CommandExecutionResult::new(command.command_id, "REJECTED"))?;
        }
        Ok(())
    }
    async fn acked(&mut self, command: RemoteCommand, accepted: bool) -> Result<()> {
        let Some((pending, session)) = self.pending.take() else {
            return Ok(());
        };
        if pending.command_id != command.command_id {
            self.pending = Some((pending, session));
            return Ok(());
        }
        if !accepted
            || session != self.status.session_id
            || self.status.paused
            || self.status.simulation
            || !self.status.backend_connected
            || !self.controller.ready()
            || command.expires_at <= Utc::now()
        {
            return self.record(CommandExecutionResult::new(command.command_id, "REJECTED"));
        }
        self.move_to(command.command_id, command.target_station)
            .await
    }
    async fn move_to(&mut self, id: Uuid, station: u8) -> Result<()> {
        let frame =
            self.controller
                .start(id, station, self.started.elapsed().as_millis() as u64)?;
        // Durable SENT before serial write. A crash on either side of write is uncertain, never replayed.
        self.record(CommandExecutionResult::new(id, "SENT"))?;
        if let Err(e) = self.write(frame).await {
            self.disconnect().await?;
            return Err(e);
        }
        Ok(())
    }
    async fn action(&mut self, action: UiAction, reply: &mut Option<Reply>) -> Result<bool> {
        let result = match action {
            UiAction::Connect { port } => {
                if self.serial.is_some() {
                    Err(Error::Safety("disconnect first"))
                } else {
                    self.status.settings.port = Some(port);
                    self.store.save_settings(&self.status.settings)?;
                    self.wants_connection = true;
                    let r = self.connect().await;
                    if r.is_err() {
                        self.disconnect().await?;
                    }
                    r
                }
            }
            UiAction::Disconnect {} => {
                self.wants_connection = false;
                self.disconnect().await
            }
            UiAction::Simulation { enabled } => {
                if self.controller.active.is_some() || self.pending.is_some() {
                    Err(Error::Safety("stop and inspect first"))
                } else {
                    self.wants_connection = false;
                    self.disconnect().await?;
                    self.status.simulation = enabled;
                    self.status.paused = true;
                    if enabled {
                        self.wants_connection = true;
                        self.connect().await
                    } else {
                        Ok(())
                    }
                }
            }
            UiAction::Simulate { control } => {
                if !self.status.simulation {
                    Err(Error::Safety("simulation disabled"))
                } else {
                    self.sim
                        .as_ref()
                        .ok_or(Error::Unavailable)?
                        .try_send(control)
                        .map_err(|_| Error::Unavailable)
                }
            }
            UiAction::ManualMove { station, confirmed } => {
                if !confirmed || !self.status.simulation {
                    Err(Error::Safety(
                        "manual motion is available only in simulator with confirmation",
                    ))
                } else {
                    let id = Uuid::new_v4();
                    self.store.receive(id, true)?;
                    self.move_to(id, station).await
                }
            }
            UiAction::Stop {} => {
                let r = self.write(Frame::Stop { v: 1 }).await;
                if let Some(event) = self.controller.uncertain("OPERATOR_STOP") {
                    self.record(event)?;
                }
                self.store.put("inspection", "true")?;
                r
            }
            UiAction::ResetFault { confirmed } => {
                if !confirmed
                    || !self.status.handshake_complete
                    || self.controller.active.is_some()
                    || !self.controller.safe
                    || self.controller.position.is_none()
                    || !self
                        .fresh_status
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(6))
                {
                    Err(Error::Safety("fault reset needs confirmed stationary sensors, released E-stop and fresh STATUS"))
                } else {
                    self.write(Frame::ResetFault { v: 1 }).await?;
                    self.write(Frame::Status { v: 1 }).await
                }
            }
            UiAction::Inspect { confirmed } => {
                if !confirmed || !self.status.handshake_complete {
                    Err(Error::Safety("explicit inspection confirmation required"))
                } else if !self
                    .fresh_status
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(6))
                {
                    self.write(Frame::Status { v: 1 }).await?;
                    Err(Error::Safety("waiting for fresh STATUS; confirm again"))
                } else {
                    self.controller.clear_inspection()?;
                    self.store.put("inspection", "false")?;
                    self.log("info", "Оператор подтвердил осмотр и безопасный STATUS");
                    Ok(())
                }
            }
            UiAction::Pause { paused } => {
                if !paused && self.status.simulation {
                    Err(Error::Safety("production intake disabled in simulator"))
                } else {
                    self.status.paused = paused;
                    Ok(())
                }
            }
            UiAction::ReconnectBackend {} => self
                .net
                .try_send(NetJob::Reconnect)
                .map_err(|_| Error::Unavailable),
            UiAction::Enroll { code } => {
                if self.status.simulation
                    || !self.status.paused
                    || self.controller.active.is_some()
                    || self.pending.is_some()
                {
                    Err(Error::Safety(
                        "pairing requires paused production intake and idle controller",
                    ))
                } else {
                    match self.net.try_send(NetJob::Enroll(
                        code,
                        reply.take().ok_or(Error::Unavailable)?,
                    )) {
                        Ok(()) => return Ok(false),
                        Err(
                            mpsc::error::TrySendError::Full(NetJob::Enroll(_, r))
                            | mpsc::error::TrySendError::Closed(NetJob::Enroll(_, r)),
                        ) => {
                            let _sent = r.send(Err(Error::Unavailable));
                            return Ok(false);
                        }
                        Err(_) => return Err(Error::Unavailable),
                    }
                }
            }
            UiAction::SaveSettings { settings } => {
                settings.validate()?;
                BackendApiClient::new(
                    &settings.backend_url,
                    cfg!(debug_assertions) && settings.allow_loopback_http,
                )?;
                if self.serial.is_some()
                    || self.pending.is_some()
                    || (self.status.enrolled
                        && settings.backend_url != self.status.settings.backend_url)
                {
                    Err(Error::Safety(
                        "disconnect serial; enrolled backend origin cannot be changed",
                    ))
                } else {
                    self.store.save_settings(&settings)?;
                    self.status.settings = settings;
                    Ok(())
                }
            }
            UiAction::Shutdown {} => {
                self.wants_connection = false;
                self.status.paused = true;
                self.disconnect().await?;
                self.publish();
                return Ok(true);
            }
        };
        if let Err(e) = &result {
            self.log("error", e.to_string());
        }
        result.map(|_| false)
    }
    async fn tick(&mut self) -> Result<()> {
        if self
            .status
            .last_heartbeat
            .is_some_and(|t| Utc::now() - t > chrono::Duration::seconds(25))
        {
            self.status.backend_connected = false;
            self.status.browser_session_id = None;
        }
        if self.binding_expiry.is_some_and(|e| e <= Utc::now()) {
            self.status.browser_session_id = None;
        }
        if let Some(event) = self
            .controller
            .tick(self.started.elapsed().as_millis() as u64)
        {
            self.record(event)?;
            if let Err(e) = self.write(Frame::Stop { v: 1 }).await {
                self.log("warn", e.to_string());
            }
        }
        if self.serial.is_some() {
            if (!self.status.handshake_complete && Instant::now() > self.handshake_deadline)
                || (self.status.handshake_complete
                    && self.last_rx.elapsed() > Duration::from_millis(3500))
            {
                self.log("error", "Serial handshake/liveness timeout");
                self.disconnect().await?;
            } else if self.status.handshake_complete
                && self.last_ping.elapsed() > Duration::from_secs(1)
            {
                self.last_ping = Instant::now();
                if self.write(Frame::Ping { v: 1 }).await.is_err() {
                    self.disconnect().await?;
                }
            }
            if self.status.handshake_complete
                && self.controller.active.is_none()
                && self.last_status.elapsed() > Duration::from_secs(4)
            {
                self.last_status = Instant::now();
                if self.write(Frame::Status { v: 1 }).await.is_err() {
                    self.disconnect().await?;
                }
            }
        } else if self.wants_connection && Instant::now() >= self.reconnect_at {
            if let Err(e) = self.connect().await {
                self.log("warn", e.to_string());
                self.disconnect().await?;
            }
        }
        Ok(())
    }
    async fn run(&mut self, mut rx: mpsc::Receiver<Message>) -> Result<()> {
        self.wants_connection =
            self.status.settings.auto_connect && self.status.settings.port.is_some();
        self.publish();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        let mut bytes = [0u8; 512];
        loop {
            enum Event {
                Message(Option<Message>),
                Read(Result<usize>),
                Tick,
            }
            let event = tokio::select! {
                message=rx.recv()=>Event::Message(message),
                n=async { match &mut self.serial { Some(s)=>s.read(&mut bytes).await,None=>std::future::pending().await } }=>Event::Read(n),
                _=tick.tick()=>Event::Tick,
            };
            match event {
                Event::Message(None) => break,
                Event::Message(Some(Message::Request(action, reply))) => {
                    let mut reply = Some(reply);
                    match self.action(action, &mut reply).await {
                        Ok(stop) => {
                            if let Some(reply) = reply {
                                let _sent = reply.send(Ok(()));
                            }
                            if stop {
                                break;
                            }
                        }
                        Err(e) => {
                            self.log("error", e.to_string());
                            if matches!(e, Error::Storage(_)) {
                                return Err(e);
                            }
                            if let Some(reply) = reply {
                                let _sent = reply.send(Err(e));
                            }
                        }
                    }
                }
                Event::Message(Some(Message::Remote(command))) => {
                    if let Err(e) = self.remote(command).await {
                        self.status.paused = true;
                        self.log("error", e.to_string());
                    }
                }
                Event::Message(Some(Message::Acked(command, accepted))) => {
                    self.acked(command, accepted).await?
                }
                Event::Message(Some(Message::Online(config))) => {
                    self.status.backend_connected = true;
                    self.status.last_heartbeat = Some(Utc::now());
                    self.status.browser_session_id = config.browser_session_id;
                    self.binding_expiry = config.browser_session_expires_at;
                    self.status.settings.station_mapping = config.station_mapping;
                }
                Event::Message(Some(Message::Identity(id))) => {
                    self.status.enrolled = id.is_some();
                    self.status.device_id = id;
                    self.store.scope = format!(
                        "{}|{}",
                        self.status.settings.backend_url,
                        id.map(|id| id.to_string()).unwrap_or_default()
                    );
                }
                Event::Message(Some(Message::Offline)) => {
                    self.status.backend_connected = false;
                    self.status.browser_session_id = None;
                    self.binding_expiry = None;
                }
                Event::Message(Some(Message::NetworkFailed)) => {
                    self.status.backend_connected = false;
                    self.status.browser_session_id = None;
                    self.status.paused = true;
                    self.log(
                        "error",
                        "Network supervisor остановлен; перезапустите приложение",
                    );
                }
                Event::Message(Some(Message::ButtonDropped)) => self.status.statistics.dropped += 1,
                Event::Message(Some(Message::Log(message))) => self.log("warn", message),
                Event::Read(Ok(0) | Err(_)) => self.disconnect().await?,
                Event::Read(Ok(n)) => {
                    for frame in self.codec.feed(&bytes[..n]) {
                        match frame {
                            Ok(frame) => self.frame(frame).await?,
                            Err(_) => {
                                self.status.statistics.invalid += 1;
                                self.log("warn", "Некорректный serial frame отброшен");
                            }
                        }
                    }
                }
                Event::Tick => self.tick().await?,
            }
            self.publish();
        }
        Ok(())
    }
}

pub fn backoff(attempt: u32) -> Duration {
    let ceiling = (500u64.saturating_mul(1u64 << attempt.min(6))).min(30000);
    Duration::from_millis(rand::rng().random_range((ceiling / 2)..=ceiling))
}
async fn notify(tx: &mpsc::Sender<Message>, message: Message) {
    if tx.send(message).await.is_err() {
        tracing::warn!("hardware actor closed");
    }
}
async fn network_worker(
    path: PathBuf,
    vault: Arc<dyn CredentialStore>,
    mut status: watch::Receiver<GatewayStatus>,
    tx: mpsc::Sender<Message>,
    mut jobs: mpsc::Receiver<NetJob>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut store = LocalEventStore::open(&path)?;
    let mut origin = status.borrow().settings.backend_url.clone();
    let mut api = BackendApiClient::new(
        &origin,
        cfg!(debug_assertions) && status.borrow().settings.allow_loopback_http,
    )?;
    let load_vault = vault.clone();
    let load_origin = origin.clone();
    let mut credentials =
        match tokio::task::spawn_blocking(move || load_vault.load(&load_origin)).await {
            Ok(Ok(c)) => c,
            _ => {
                notify(
                    &tx,
                    Message::Log("Credential Manager недоступен или origin изменён"),
                )
                .await;
                None
            }
        };
    notify(
        &tx,
        Message::Identity(credentials.as_ref().map(|c| c.device_id)),
    )
    .await;
    let mut token: Option<AccessToken> = None;
    let mut next = Instant::now();
    let mut attempt = 0;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
    let mut config: Option<DeviceConfig> = None;
    let mut poll: Option<tokio::task::JoinHandle<Result<Option<RemoteCommand>>>> = None;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let snapshot = status.borrow().clone();
        if snapshot.settings.backend_url != origin {
            origin = snapshot.settings.backend_url.clone();
            api = BackendApiClient::new(
                &origin,
                cfg!(debug_assertions) && snapshot.settings.allow_loopback_http,
            )?;
            token = None;
            credentials = None;
            config = None;
            notify(&tx, Message::Identity(None)).await;
        }
        if token
            .as_ref()
            .is_some_and(|t| t.expires_at <= Utc::now() + chrono::Duration::seconds(5))
        {
            token = None;
            notify(&tx, Message::Offline).await;
        }
        if credentials.is_some()
            && token.is_none()
            && Instant::now() >= next
            && !snapshot.simulation
        {
            let auth = tokio::select! { _=shutdown.changed()=>break,r=api.authenticate(credentials.as_ref().ok_or(Error::Credentials)?)=>r };
            match auth {
                Ok(t) => {
                    token = Some(t);
                    attempt = 0;
                    next = Instant::now();
                }
                Err(e) => {
                    notify(&tx, Message::Offline).await;
                    notify(
                        &tx,
                        Message::Log("Device auth недоступен / credentials отозваны"),
                    )
                    .await;
                    if matches!(e, Error::Http(401 | 403)) {
                        credentials = None;
                    }
                    next = Instant::now() + backoff(attempt);
                    attempt = attempt.saturating_add(1);
                }
            }
        }
        let can_poll = token.is_some()
            && config.is_some()
            && !snapshot.paused
            && !snapshot.simulation
            && snapshot.backend_connected
            && snapshot.active_command.is_none()
            && Instant::now() >= next;
        if !can_poll {
            if let Some(task) = poll.take() {
                task.abort();
            }
        }
        if can_poll && poll.is_none() {
            let client = api.clone();
            let access = token
                .as_ref()
                .ok_or(Error::Credentials)?
                .access_token
                .clone();
            poll = Some(tokio::spawn(
                async move { client.next_command(&access).await },
            ));
        }
        enum Event {
            Job(Option<NetJob>),
            Poll(Result<Option<RemoteCommand>>),
            Beat,
            Wake,
            Shutdown,
        }
        let event = tokio::select! {
            _=shutdown.changed()=>Event::Shutdown,
            job=jobs.recv()=>Event::Job(job),
            r=async { match &mut poll {Some(task)=>task.await.map_err(|_|Error::Unavailable)?,None=>std::future::pending().await} }=>Event::Poll(r),
            _=heartbeat.tick()=>Event::Beat,
            _=tokio::time::sleep(Duration::from_millis(500))=>Event::Wake,
            _=status.changed()=>Event::Wake,
        };
        let mut error = None;
        match event {
            Event::Shutdown | Event::Job(None) => break,
            Event::Job(Some(NetJob::Reconnect)) => {
                token = None;
                config = None;
                next = Instant::now();
            }
            Event::Job(Some(NetJob::Enroll(code, reply))) => {
                let result = async {
                    let c = api.enroll(&code).await?;
                    let v = vault.clone();
                    let c = tokio::task::spawn_blocking(move || {
                        v.save(&c)?;
                        Ok::<_, Error>(c)
                    })
                    .await
                    .map_err(|_| Error::Credentials)??;
                    credentials = Some(c);
                    token = None;
                    config = None;
                    next = Instant::now();
                    notify(
                        &tx,
                        Message::Identity(credentials.as_ref().map(|c| c.device_id)),
                    )
                    .await;
                    Ok(())
                }
                .await;
                let _sent = reply.send(result);
            }
            Event::Job(Some(NetJob::Ack(command))) => {
                let result = if let Some(t) = &token {
                    api.ack(&t.access_token, &command).await
                } else {
                    Err(Error::Unavailable)
                };
                let accepted = result.is_ok();
                if let Err(e) = result {
                    error = Some(e);
                }
                notify(&tx, Message::Acked(command, accepted)).await;
            }
            Event::Job(Some(NetJob::Button(event))) => {
                if event.expires_at <= Utc::now()
                    || snapshot.simulation
                    || event.session_id != snapshot.session_id
                    || !snapshot.backend_connected
                {
                    notify(&tx, Message::ButtonDropped).await;
                } else if let Some(t) = &token {
                    let ttl = (event.expires_at - Utc::now())
                        .to_std()
                        .unwrap_or(Duration::ZERO);
                    match tokio::time::timeout(ttl, api.button(&t.access_token, &event)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            error = Some(e);
                            notify(&tx, Message::ButtonDropped).await;
                        }
                        Err(_) => notify(&tx, Message::ButtonDropped).await,
                    }
                } else {
                    notify(&tx, Message::ButtonDropped).await;
                }
            }
            Event::Poll(Ok(Some(command))) => {
                poll = None;
                notify(&tx, Message::Remote(command)).await;
                next = Instant::now() + Duration::from_millis(500);
            }
            Event::Poll(Ok(None)) => {
                poll = None;
                next = Instant::now() + Duration::from_millis(300);
            }
            Event::Poll(Err(e)) => {
                poll = None;
                error = Some(e);
            }
            Event::Beat if !snapshot.simulation => {
                if let (Some(t), Some(c)) = (&token, &credentials) {
                    let result = async {
                        let cfg = api.config(&t.access_token, c.device_id).await?;
                        api.heartbeat(&t.access_token, &snapshot).await?;
                        api.telemetry(&t.access_token, &snapshot).await?;
                        notify(&tx, Message::Online(cfg.clone())).await;
                        config = Some(cfg);
                        let scope = format!("{}|{}", origin, c.device_id);
                        for event in store.outbox(&scope)? {
                            api.publish(&t.access_token, &event).await?;
                            store.delivered(event.event_id)?;
                        }
                        Ok::<_, Error>(())
                    }
                    .await;
                    if let Err(e) = result {
                        error = Some(e);
                    }
                }
            }
            _ => {}
        }
        if let Some(e) = error {
            tracing::warn!(error=%e,"device API request failed");
            notify(&tx, Message::Offline).await;
            config = None;
            if matches!(e, Error::Http(401 | 403)) {
                token = None;
            }
            notify(
                &tx,
                Message::Log("Device API недоступен; доставка не подтверждена"),
            )
            .await;
            next = Instant::now() + backoff(attempt);
            attempt = attempt.saturating_add(1);
        }
    }
    if let Some(task) = poll {
        task.abort();
    }
    Ok(())
}
