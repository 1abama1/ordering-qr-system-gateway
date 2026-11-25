use gateway_core::{
    backend::{CredentialStore, DeviceCredentials},
    domain::*,
    runtime::UiAction,
    simulator::{Scenario, SimControl},
    GatewayHandle, GatewaySupervisor,
};
use std::sync::Arc;
struct EmptyVault;
impl CredentialStore for EmptyVault {
    fn load(&self, _: &str) -> Result<Option<DeviceCredentials>> {
        Ok(None)
    }
    fn save(&self, _: &DeviceCredentials) -> Result<()> {
        Err(Error::Credentials)
    }
}
async fn wait(handle: &GatewayHandle, predicate: impl Fn(&GatewayStatus) -> bool) {
    let mut status = handle.status.clone();
    tokio::time::timeout(std::time::Duration::from_secs(6), async {
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
async fn simulator_buttons_motion_loss_reconnect_inspection_and_shutdown() {
    let temp = tempfile::tempdir().expect("temp");
    let (handle, task) =
        GatewaySupervisor::start(temp.path().join("runtime.db"), Arc::new(EmptyVault))
            .expect("start");
    handle
        .execute(UiAction::Simulation { enabled: true })
        .await
        .expect("sim");
    wait(&handle, |s| s.state == ConveyorState::Ready).await;
    for button in [HardwareButton::Up, HardwareButton::Yes] {
        handle
            .execute(UiAction::Simulate {
                control: SimControl::Button { button },
            })
            .await
            .expect("button");
        wait(&handle, |s| {
            s.buttons
                .iter()
                .any(|e| e.button == button && e.edge == ButtonEdge::Release)
        })
        .await;
    }
    assert!(!handle.snapshot().backend_connected);
    assert!(handle.snapshot().buttons.iter().all(|e| e.simulation));
    let before = handle.snapshot().statistics.button_events;
    for button in [
        HardwareButton::Up,
        HardwareButton::Down,
        HardwareButton::Left,
        HardwareButton::Right,
        HardwareButton::Yes,
        HardwareButton::No,
    ] {
        handle
            .execute(UiAction::Simulate {
                control: SimControl::Button { button },
            })
            .await
            .expect("independent simultaneous buttons");
    }
    wait(&handle, |s| s.statistics.button_events == before + 12).await;
    assert!(handle
        .execute(UiAction::Pause { paused: false })
        .await
        .is_err());
    handle
        .execute(UiAction::ManualMove {
            station: 4,
            confirmed: true,
        })
        .await
        .expect("move");
    wait(&handle, |s| s.state == ConveyorState::Arrived).await;
    assert_eq!(handle.snapshot().position, Some(4));
    handle
        .execute(UiAction::Simulate {
            control: SimControl::Scenario {
                kind: Scenario::LostAck,
            },
        })
        .await
        .expect("lost ack");
    handle
        .execute(UiAction::ManualMove {
            station: 2,
            confirmed: true,
        })
        .await
        .expect("move");
    wait(&handle, |s| {
        s.history.iter().any(|e| e.kind == "NEEDS_INSPECTION")
    })
    .await;
    assert!(!handle.snapshot().safe);
    handle
        .execute(UiAction::Disconnect {})
        .await
        .expect("disconnect");
    handle
        .execute(UiAction::Simulation { enabled: true })
        .await
        .expect("reconnect");
    wait(&handle, |s| s.handshake_complete && s.position == Some(1)).await;
    assert_eq!(handle.snapshot().state, ConveyorState::NeedsInspection);
    assert!(handle
        .execute(UiAction::ManualMove {
            station: 3,
            confirmed: true
        })
        .await
        .is_err());
    handle
        .execute(UiAction::Inspect { confirmed: true })
        .await
        .expect("inspect");
    wait(&handle, |s| s.state == ConveyorState::Ready).await;
    handle
        .execute(UiAction::Shutdown {})
        .await
        .expect("shutdown");
    assert!(task.await.expect("join").is_ok());
}
#[test]
fn ipc_rejects_arbitrary_serial_and_unknown_actions() {
    for text in [
        r#"{"type":"serial_write","data":"MOVE_TO"}"#,
        r#"{"type":"stop","extra":true}"#,
        r#"{"type":"manual_move","station":999,"confirmed":true}"#,
    ] {
        assert!(serde_json::from_str::<UiAction>(text).is_err());
    }
}
