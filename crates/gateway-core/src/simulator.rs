use crate::{
    domain::*,
    ports::SerialTransport,
    protocol::{Frame, SerialProtocolCodec},
};
use async_trait::async_trait;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::mpsc,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SimControl {
    Button { button: HardwareButton },
    Scenario { kind: Scenario },
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    Fault,
    EStop,
    LostAck,
    Duplicate,
    InvalidJson,
    Delayed,
    Reset,
    Disconnect,
    SensorFailure,
}
pub struct SimulatedSerialTransport(DuplexStream);
#[async_trait]
impl SerialTransport for SimulatedSerialTransport {
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize> {
        Ok(self.0.read(bytes).await?)
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).await?;
        Ok(())
    }
}
pub fn connect() -> (
    SimulatedSerialTransport,
    mpsc::Sender<SimControl>,
    tokio::task::JoinHandle<Result<()>>,
) {
    let (host, device) = tokio::io::duplex(4096);
    let (tx, rx) = mpsc::channel(32);
    (
        SimulatedSerialTransport(host),
        tx,
        tokio::spawn(run(device, rx)),
    )
}
async fn send(device: &mut DuplexStream, frame: Frame) -> Result<()> {
    device.write_all(&frame.encode()?).await?;
    Ok(())
}
async fn run(mut device: DuplexStream, mut rx: mpsc::Receiver<SimControl>) -> Result<()> {
    let mut codec = SerialProtocolCodec::default();
    let mut buffer = [0u8; 512];
    let mut seq = 0;
    let mut station = Some(1);
    let mut state = ConveyorState::Ready;
    let mut active: Option<(Uuid, u8, Instant)> = None;
    let mut release: Vec<(HardwareButton, Instant)> = Vec::with_capacity(6);
    let mut lost_ack = false;
    let mut duplicate = false;
    let mut sensor_failure = false;
    let mut delayed = false;
    let mut hello = false;
    let mut last_id: Option<Uuid> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    loop {
        tokio::select! {
            n = device.read(&mut buffer) => {
                let n = n?; if n == 0 { break; }
                for frame in codec.feed(&buffer[..n]).into_iter().flatten() {
                    match frame {
                        Frame::Hello{nonce,..} => { hello=true; seq=0; send(&mut device, Frame::HelloAck{v:1,nonce,fw:"1.0.0".into(),cap:vec!["BUTTONS".into(),"CONVEYOR".into()]}).await?; }
                        Frame::Status{..} if hello => { send(&mut device,Frame::State{v:1,state,station,safe:matches!(state,ConveyorState::Ready|ConveyorState::Arrived),command_id:active.map(|a|a.0)}).await?; }
                        Frame::Ping{..} if hello => { send(&mut device,Frame::Pong{v:1}).await?; }
                        Frame::MoveTo{command_id,station:target,..} if hello => {
                            if last_id == Some(command_id) { send(&mut device,Frame::Ack{v:1,command_id}).await?; continue; }
                            if active.is_some() || !matches!(state,ConveyorState::Ready|ConveyorState::Arrived) { send(&mut device,Frame::Fault{v:1,code:"BUSY".into()}).await?; continue; }
                            last_id=Some(command_id); state=ConveyorState::Moving;station=None;
                            if !lost_ack { send(&mut device,Frame::Ack{v:1,command_id}).await?; }
                            lost_ack=false;
                            send(&mut device,Frame::Moving{v:1,command_id}).await?;
                            active=Some((command_id,target,Instant::now()+Duration::from_millis(if delayed { 17000 } else { 900 })));
                            delayed=false;
                        }
                        Frame::Stop{..} => { active=None; state=ConveyorState::Fault; send(&mut device,Frame::Fault{v:1,code:"STOPPED".into()}).await?; }
                        Frame::ResetFault{..} => {
                            if active.is_none() && station.is_some() && state != ConveyorState::EStop { state=ConveyorState::Ready; }
                            send(&mut device,Frame::State{v:1,state,station,safe:state==ConveyorState::Ready,command_id:None}).await?;
                        }
                        _ => {}
                    }
                }
            }
            control = rx.recv() => {
                match control {
                    None => break,
                    Some(SimControl::Button{button}) if hello => {
                        if release.iter().any(|(held,_)|*held==button) { continue; }
                        seq+=1; let frame=Frame::Button{v:1,seq,b:button,edge:ButtonEdge::Press};
                        send(&mut device,frame.clone()).await?;
                        if duplicate { send(&mut device,frame).await?;duplicate=false; }
                        release.push((button,Instant::now()+Duration::from_millis(200)));
                    }
                    Some(SimControl::Scenario{kind}) => match kind {
                        Scenario::LostAck => { lost_ack=true;sensor_failure=true; },
                        Scenario::Duplicate => duplicate=true,
                        Scenario::InvalidJson => { device.write_all(b"{broken-json}\n").await?; },
                        Scenario::Delayed => delayed=true,
                        Scenario::SensorFailure => sensor_failure=true,
                        Scenario::Disconnect|Scenario::Reset => break,
                        Scenario::Fault|Scenario::EStop => {
                            active=None;state=if matches!(kind,Scenario::EStop) { ConveyorState::EStop } else { ConveyorState::Fault };
                            send(&mut device,Frame::Fault{v:1,code:if matches!(kind,Scenario::EStop) { "E_STOP" } else { "SENSOR_FAILURE" }.into()}).await?;
                        }
                    },
                    _ => {}
                }
            }
            _ = tick.tick() => {
                let due_buttons:Vec<_>=release.iter().filter(|(_,due)|Instant::now()>=*due).map(|(button,_)|*button).collect();
                release.retain(|(button,_)|!due_buttons.contains(button));
                for button in due_buttons {seq+=1;send(&mut device,Frame::Button{v:1,seq,b:button,edge:ButtonEdge::Release}).await?;}
                if let Some((id,target,due))=active {
                    if Instant::now()>=due && !sensor_failure {
                        active=None;state=ConveyorState::Arrived;station=Some(target);
                        send(&mut device,Frame::Arrived{v:1,command_id:id,station:target}).await?;
                    }
                }
            }
        }
    }
    Ok(())
}
