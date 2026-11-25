//! Full supervisor with an injected serial fixture and a loopback mock backend.
//! The fixture cannot be selected by desktop IPC or production settings.
use async_trait::async_trait;
use chrono::Utc;
use gateway_core::{
    backend::{CredentialStore, DeviceCredentials},
    domain::*,
    persistence::LocalEventStore,
    ports::{SerialConnector, SerialTransport},
    runtime::UiAction,
    simulator::{self, SimControl, SimulatedSerialTransport},
    GatewayHandle, GatewaySupervisor,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;
use wiremock::{
    matchers::{body_partial_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};
struct Vault {
    id: Uuid,
    origin: String,
}
impl CredentialStore for Vault {
    fn load(&self, _: &str) -> Result<Option<DeviceCredentials>> {
        Ok(Some(DeviceCredentials {
            device_id: self.id,
            origin: self.origin.clone(),
            secret: "mock-fixture-credential".into(),
        }))
    }
    fn save(&self, _: &DeviceCredentials) -> Result<()> {
        Err(Error::Credentials)
    }
}
struct Fixture {
    controls: watch::Sender<Option<mpsc::Sender<SimControl>>>,
}
struct Transport {
    serial: SimulatedSerialTransport,
    task: tokio::task::JoinHandle<Result<()>>,
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[async_trait]
impl SerialTransport for Transport {
    async fn read(&mut self, b: &mut [u8]) -> Result<usize> {
        self.serial.read(b).await
    }
    async fn write(&mut self, b: &[u8]) -> Result<()> {
        self.serial.write(b).await
    }
}
#[async_trait]
impl SerialConnector for Fixture {
    async fn open(&self, _: &str, _: u32) -> Result<Box<dyn SerialTransport>> {
        let (serial, control, task) = simulator::connect();
        self.controls.send_replace(Some(control));
        Ok(Box::new(Transport { serial, task }))
    }
}
async fn wait(handle: &GatewayHandle, predicate: impl Fn(&GatewayStatus) -> bool) {
    let mut status = handle.status.clone();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if predicate(&status.borrow()) {
                break;
            }
            status.changed().await.expect("running");
        }
    })
    .await
    .expect("state timeout");
}
#[tokio::test]
async fn authenticated_supervisor_routes_buttons_and_executes_duplicate_command_once() {
    let server = MockServer::start().await;
    let device = Uuid::new_v4();
    let id = Uuid::new_v4();
    let browser = Uuid::new_v4();
    let command = RemoteCommand {
        contract: "hardware.v1".into(),
        command_id: id,
        device_id: device,
        action: "MOVE_TO".into(),
        target_station: 4,
        order_id: 42,
        issued_at: Utc::now() - chrono::Duration::seconds(1),
        expires_at: Utc::now() + chrono::Duration::seconds(60),
        lease_token: "fixture-fence".into(),
        attempt: 1,
        version: 1,
    };
    Mock::given(path("/api/device/auth/token")).and(body_partial_json(serde_json::json!({"deviceId":device,"deviceSecret":"mock-fixture-credential"}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"contract":"hardware.v1","accessToken":"fixture-access-token","expiresAt":Utc::now()+chrono::Duration::minutes(5),"scopes":["hardware.v1"]}))).mount(&server).await;
    Mock::given(path("/api/device/config")).and(header("Authorization","Bearer fixture-access-token")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"contract":"hardware.v1","deviceId":device,"stationMapping":[2,1,3,4,5,6],"browserSessionId":browser,"browserSessionExpiresAt":Utc::now()+chrono::Duration::minutes(1)}))).mount(&server).await;
    Mock::given(path("/api/device/heartbeat"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(path("/api/device/telemetry"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(path("/api/device/buttons/events")).and(body_partial_json(serde_json::json!({"button":"YES","deviceId":device,"browserSessionId":browser,"simulation":false}))).respond_with(ResponseTemplate::new(204)).expect(2).mount(&server).await;
    Mock::given(path("/api/device/commands/next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&command))
        .mount(&server)
        .await;
    Mock::given(path(format!("/api/device/commands/{id}/ack")))
        .and(body_partial_json(
            serde_json::json!({"version":1,"leaseToken":"fixture-fence"}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/api/device/commands/{id}/events")))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().expect("temp");
    let db = temp.path().join("workflow.db");
    let settings = Settings {
        backend_url: server.uri(),
        allow_loopback_http: true,
        port: Some("FIXTURE".into()),
        auto_connect: true,
        ..Settings::default()
    };
    LocalEventStore::open(&db)
        .expect("store")
        .save_settings(&settings)
        .expect("settings");
    let (control_tx, control_rx) = watch::channel(None);
    let (handle, task) = GatewaySupervisor::start_with_connector(
        db,
        Arc::new(Vault {
            id: device,
            origin: server.uri(),
        }),
        Arc::new(Fixture {
            controls: control_tx,
        }),
    )
    .expect("start");
    wait(&handle, |s| {
        s.backend_connected && s.safe && s.browser_session_id == Some(browser)
    })
    .await;
    assert_eq!(
        handle.snapshot().settings.station_mapping,
        [2, 1, 3, 4, 5, 6]
    );
    let control = control_rx.borrow().clone().expect("fixture");
    control
        .send(SimControl::Button {
            button: HardwareButton::Yes,
        })
        .await
        .expect("button");
    wait(&handle, |s| s.statistics.button_events >= 2).await;
    handle
        .execute(UiAction::Pause { paused: false })
        .await
        .expect("resume");
    wait(&handle, |s| {
        s.history
            .iter()
            .any(|e| e.command_id == id && e.kind == "ARRIVED")
    })
    .await;
    tokio::time::timeout(Duration::from_secs(16), async {
        loop {
            let requests = server.received_requests().await.expect("requests");
            if requests.iter().any(|r| {
                r.url.path().ends_with("/events")
                    && serde_json::from_slice::<serde_json::Value>(&r.body)
                        .is_ok_and(|v| v["kind"] == "ARRIVED")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("durable result delivery");
    assert_eq!(
        handle
            .snapshot()
            .history
            .iter()
            .filter(|e| e.kind == "SENT" && e.command_id == id)
            .count(),
        1
    );
    assert!(handle
        .snapshot()
        .logs
        .iter()
        .any(|l| l.message.contains("Повтор команды")));
    handle
        .execute(UiAction::Shutdown {})
        .await
        .expect("shutdown");
    assert!(task.await.expect("join").is_ok());
    server.verify().await;
}
