use chrono::{Duration, Utc};
use gateway_core::{
    controller::ConveyorController,
    domain::*,
    input::HardwareInputService,
    persistence::LocalEventStore,
    ports::Persistence,
    protocol::{self, Frame, SerialProtocolCodec},
    runtime::backoff,
};
use uuid::Uuid;
fn ready() -> ConveyorController {
    let mut c = ConveyorController::default();
    c.connected();
    c.frame(&Frame::State {
        v: 1,
        state: ConveyorState::Ready,
        station: Some(1),
        safe: true,
        command_id: None,
    });
    c
}
#[test]
fn up_and_yes_have_independent_press_release() {
    let mut input = HardwareInputService::default();
    let session = Uuid::new_v4();
    input.reset(session);
    let device = Uuid::new_v4();
    for (seq, button, edge) in [
        (1, HardwareButton::Up, ButtonEdge::Press),
        (2, HardwareButton::Yes, ButtonEdge::Press),
        (3, HardwareButton::Up, ButtonEdge::Release),
        (4, HardwareButton::Yes, ButtonEdge::Release),
    ] {
        let e = input
            .accept(device, session, seq, button, edge, Utc::now(), false)
            .expect("independent edge");
        assert_eq!(e.expires_at - e.occurred_at, Duration::milliseconds(1500));
        assert_eq!(e.browser_session_id, None);
    }
}
#[test]
fn duplicate_hold_old_sequence_and_old_session_rejected() {
    let mut input = HardwareInputService::default();
    let session = Uuid::new_v4();
    input.reset(session);
    let device = Uuid::new_v4();
    assert!(input
        .accept(
            device,
            session,
            1,
            HardwareButton::Yes,
            ButtonEdge::Press,
            Utc::now(),
            false
        )
        .is_some());
    for seq in [1, 2, 0] {
        assert!(input
            .accept(
                device,
                session,
                seq,
                HardwareButton::Yes,
                ButtonEdge::Press,
                Utc::now(),
                false
            )
            .is_none());
    }
    input.reset(Uuid::new_v4());
    assert!(input
        .accept(
            device,
            session,
            3,
            HardwareButton::Yes,
            ButtonEdge::Release,
            Utc::now(),
            false
        )
        .is_none());
}
#[test]
fn ack_is_not_arrival() {
    let mut c = ready();
    let id = Uuid::new_v4();
    c.start(id, 4, 0).expect("start");
    assert_eq!(
        c.frame(&Frame::Ack {
            v: 1,
            command_id: id
        })
        .expect("ack")
        .kind,
        "ACKNOWLEDGED"
    );
    assert_eq!(c.state, ConveyorState::Moving);
    assert!(!c.ready());
    assert!(c.start(Uuid::new_v4(), 2, 10).is_err());
    assert_eq!(
        c.frame(&Frame::Arrived {
            v: 1,
            command_id: id,
            station: 4
        })
        .expect("arrived")
        .kind,
        "ARRIVED"
    );
    assert_eq!(c.position, Some(4));
}
#[test]
fn mismatched_arrival_cannot_complete() {
    let mut c = ready();
    c.start(Uuid::new_v4(), 4, 0).expect("start");
    assert_eq!(
        c.frame(&Frame::Arrived {
            v: 1,
            command_id: Uuid::new_v4(),
            station: 4
        })
        .expect("uncertain")
        .kind,
        "NEEDS_INSPECTION"
    );
    assert_eq!(c.position, None);
}
#[test]
fn usb_loss_latches_inspection_across_reconnect() {
    let mut c = ready();
    c.start(Uuid::new_v4(), 4, 0).expect("start");
    assert!(c.disconnected().is_some());
    c.connected();
    c.frame(&Frame::State {
        v: 1,
        state: ConveyorState::Ready,
        station: Some(2),
        safe: true,
        command_id: None,
    });
    assert!(!c.ready());
    assert_eq!(c.state, ConveyorState::NeedsInspection);
    c.clear_inspection().expect("explicit inspection");
    assert!(c.ready());
}
#[test]
fn lost_ack_and_sensor_timeout_never_succeed() {
    for acked in [false, true] {
        let mut c = ready();
        let id = Uuid::new_v4();
        c.start(id, 3, 0).expect("start");
        if acked {
            c.frame(&Frame::Ack {
                v: 1,
                command_id: id,
            });
        }
        assert_eq!(
            c.tick(if acked { 15000 } else { 2000 })
                .expect("timeout")
                .kind,
            "NEEDS_INSPECTION"
        );
        assert!(!c.ready());
    }
}
#[test]
fn status_cannot_finish_active_movement() {
    let mut c = ready();
    let id = Uuid::new_v4();
    c.start(id, 4, 0).expect("start");
    assert_eq!(
        c.frame(&Frame::State {
            v: 1,
            state: ConveyorState::Ready,
            station: Some(4),
            safe: true,
            command_id: Some(id)
        })
        .expect("uncertain")
        .kind,
        "NEEDS_INSPECTION"
    );
}
#[test]
fn estop_cannot_be_cleared_by_operator_alone() {
    let mut c = ready();
    c.frame(&Frame::Fault {
        v: 1,
        code: "E_STOP".into(),
    });
    assert!(c.clear_inspection().is_err());
    c.frame(&Frame::State {
        v: 1,
        state: ConveyorState::Fault,
        station: Some(1),
        safe: true,
        command_id: None,
    });
    assert!(c.clear_inspection().is_err());
}
#[test]
fn codec_recovers_from_oversize_invalid_and_split_utf8() {
    let mut codec = SerialProtocolCodec::default();
    assert!(codec.feed(b"{\"v\":1,\"t\":\"PO").is_empty());
    assert!(matches!(&codec.feed(b"NG\"}\n")[0], Ok(Frame::Pong { .. })));
    let mut bad = vec![b'x'; 300];
    bad.extend_from_slice(b"\n{invalid}\n{\"v\":1,\"t\":\"PONG\"}\n");
    let results = codec.feed(&bad);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_err());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    assert!(protocol::parse(&[255]).is_err());
}
#[test]
fn strict_fields_versions_ranges_and_ids() {
    for text in [
        r#"{"v":2,"t":"PONG"}"#,
        r#"{"v":1,"t":"MOVE_TO","commandId":"bad","station":1}"#,
        r#"{"v":1,"t":"BTN","seq":1,"b":"YES","edge":"PRESS","extra":true}"#,
        r#"{"v":1,"t":"BTN","seq":-1,"b":"YES","edge":"PRESS"}"#,
        r#"{"v":1,"t":"MOVE_TO","commandId":"00000000-0000-0000-0000-000000000000","station":1}"#,
    ] {
        assert!(protocol::parse(text.as_bytes()).is_err(), "{text}");
    }
}
#[test]
fn restart_recovery_is_durable_and_idempotent() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("ledger.db");
    let id = Uuid::new_v4();
    {
        let mut db = LocalEventStore::open(&path).expect("db");
        db.scope = "origin|device".into();
        assert!(db.receive(id, false).expect("receive"));
        assert!(!db.receive(id, false).expect("duplicate"));
        db.record(&CommandExecutionResult::new(id, "SENT"), false)
            .expect("sent");
    }
    let mut db = LocalEventStore::open(&path).expect("reopen");
    assert_eq!(db.recover().expect("recovery")[0].kind, "NEEDS_INSPECTION");
    assert!(db.inspection().expect("inspection"));
    assert!(db.recover().expect("idempotent").is_empty());
    assert!(!db.receive(id, false).expect("never replay"));
    let events = db.outbox("origin|device").expect("outbox");
    assert_eq!(events.len(), 2);
    assert!(db.outbox("other|device").expect("partition").is_empty());
    for e in events {
        db.delivered(e.event_id).expect("delivered");
    }
    assert_eq!(db.history().expect("journal").len(), 2);
}
#[test]
fn simulated_results_never_enter_production_outbox() {
    let mut db = LocalEventStore::open(std::path::Path::new(":memory:")).expect("db");
    let id = Uuid::new_v4();
    db.receive(id, true).expect("receive");
    db.record(&CommandExecutionResult::new(id, "ARRIVED"), true)
        .expect("record");
    assert!(db.outbox("").expect("outbox").is_empty());
}
#[test]
fn command_deadlines_device_and_version_validated() {
    let device = Uuid::new_v4();
    let now = Utc::now();
    let mut c = RemoteCommand {
        contract: "hardware.v1".into(),
        command_id: Uuid::new_v4(),
        device_id: device,
        action: "MOVE_TO".into(),
        target_station: 1,
        order_id: 1,
        issued_at: now - Duration::seconds(1),
        expires_at: now + Duration::seconds(5),
        lease_token: "fence".into(),
        attempt: 1,
        version: 1,
    };
    assert!(c.validate(device, now).is_ok());
    assert!(c.validate(Uuid::new_v4(), now).is_err());
    c.expires_at = now;
    assert!(c.validate(device, now).is_err());
    c.version = 2;
    assert!(c.validate(device, now - Duration::seconds(1)).is_err());
}
#[test]
fn jitter_is_bounded() {
    for attempt in 0..100 {
        let delay = backoff(attempt);
        assert!(delay >= std::time::Duration::from_millis(250));
        assert!(delay <= std::time::Duration::from_secs(30));
    }
}

#[test]
fn unsupported_firmware_and_ready_without_sensor_are_rejected() {
    assert!(protocol::supported_firmware("1.0.0"));
    for fw in ["2.0.0", "1.1.0", "1.0.bad", "1.0.", "1.0.0-dev"] {
        assert!(!protocol::supported_firmware(fw));
    }
    let mut c = ConveyorController::default();
    c.connected();
    c.frame(&Frame::State {
        v: 1,
        state: ConveyorState::Ready,
        station: None,
        safe: true,
        command_id: None,
    });
    assert_eq!(c.state, ConveyorState::NeedsInspection);
    assert!(!c.ready());
}
proptest::proptest! {
 #[test]fn arbitrary_serial_input_does_not_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY,0..16384)) {let mut codec=SerialProtocolCodec::default();for chunk in bytes.chunks(31) {let _frames=codec.feed(chunk);}let frames=codec.feed(b"\n{\"v\":1,\"t\":\"PONG\"}\n");proptest::prop_assert!(matches!(frames.last(),Some(Ok(Frame::Pong{..}))),"codec resynchronizes");}
}
