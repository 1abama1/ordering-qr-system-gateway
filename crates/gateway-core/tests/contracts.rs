use chrono::{Duration, Utc};
use gateway_core::{
    backend::{BackendApiClient, BackendTransport, DeviceCredentials},
    domain::*,
    ports::SerialTransport,
    protocol::{Frame, SerialProtocolCodec},
    simulator,
};
use secrecy::SecretString;
use uuid::Uuid;
use wiremock::{
    matchers::{body_json, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};
fn client(server: &MockServer) -> BackendApiClient {
    BackendApiClient::new(&server.uri(), true).expect("loopback mock")
}
#[tokio::test]
async fn hardware_heartbeat_reports_serial_observations_and_rejects_simulation() {
    let server = MockServer::start().await;
    let api = client(&server);
    let id = Uuid::new_v4();
    let mut status = GatewayStatus {
        device_id: Some(id),
        state: ConveyorState::Ready,
        usb_connected: true,
        handshake_complete: true,
        firmware: Some("1.0.2".into()),
        capabilities: vec!["BUTTONS".into(), "CONVEYOR".into()],
        safe: true,
        position: Some(4),
        paused: false,
        ..GatewayStatus::default()
    };
    Mock::given(method("POST"))
        .and(path("/api/device/heartbeat"))
        .and(body_json(serde_json::json!({
            "contract":"hardware.v1","deviceId":id,"sessionId":status.session_id,"state":"READY",
            "paused":false,"simulation":false,"serialConnected":true,"arduinoDetected":true,
            "firmwareVersion":"1.0.2","protocolVersion":1,"safe":true,"currentPosition":4,
            "activeCommandId":null,"buttonControllerOnline":true,"faultCode":null,
            "capabilities":["BUTTONS","CONVEYOR"],"supportedActions":["MOVE_TO"]
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let token = "test-only-token".into();
    assert!(api.heartbeat(&token, &status).await.is_ok());
    status.simulation = true;
    assert!(matches!(
        api.heartbeat(&token, &status).await,
        Err(Error::Safety(_))
    ));
}
#[test]
fn production_rejects_insecure_origins_and_url_secrets() {
    for origin in [
        "http://restaurant.example",
        "https://user:secret@example.com",
        "https://example.com?token=x",
        "https://example.com/path",
        "https://example.com/#secret",
        "file:///etc/passwd",
    ] {
        assert!(BackendApiClient::new(origin, false).is_err(), "{origin}");
    }
    assert!(BackendApiClient::new("http://127.0.0.1:1234", false).is_err());
    assert!(BackendApiClient::new("http://127.0.0.1:1234", true).is_ok());
}
#[tokio::test]
async fn enrollment_and_scoped_token_contract() {
    let server = MockServer::start().await;
    let id = Uuid::new_v4();
    Mock::given(method("POST")).and(path("/api/device/enroll")).and(body_json(serde_json::json!({"contract":"hardware.v1","pairingCode":"ABC123"}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"contract":"hardware.v1","deviceId":id,"deviceSecret":"mock-device-secret-123456"}))).expect(1).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/device/auth/token")).and(body_json(serde_json::json!({"contract":"hardware.v1","deviceId":id,"deviceSecret":"mock-device-secret-123456"}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"contract":"hardware.v1","accessToken":"mock-access-token","expiresAt":Utc::now()+Duration::minutes(5),"scopes":["hardware.v1"]}))).expect(1).mount(&server).await;
    let api = client(&server);
    let c = api.enroll("ABC123").await.expect("enroll");
    assert_eq!(c.device_id, id);
    assert!(api.authenticate(&c).await.is_ok());
}
#[tokio::test]
async fn token_revocation_expiry_and_missing_api_are_not_success() {
    let server = MockServer::start().await;
    let api = client(&server);
    let c = DeviceCredentials {
        device_id: Uuid::new_v4(),
        secret: "mock-device-secret".into(),
        origin: server.uri(),
    };
    Mock::given(path("/api/device/auth/token"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    assert!(matches!(api.authenticate(&c).await, Err(Error::Http(401))));
    server.reset().await;
    Mock::given(path("/api/device/auth/token")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"contract":"hardware.v1","accessToken":"mock-token","expiresAt":Utc::now()-Duration::seconds(1),"scopes":["hardware.v1"]}))).mount(&server).await;
    assert!(api.authenticate(&c).await.is_err());
    assert!(matches!(
        api.config(&"mock-token".into(), c.device_id).await,
        Err(Error::Http(404))
    ));
}
#[tokio::test]
async fn button_ttl_and_absent_browser_are_enforced_before_http() {
    let server = MockServer::start().await;
    let api = client(&server);
    let mut event = HardwareEvent {
        contract: "hardware.v1".into(),
        event_id: Uuid::new_v4(),
        device_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        seq: 1,
        button: HardwareButton::Yes,
        edge: ButtonEdge::Press,
        occurred_at: Utc::now(),
        expires_at: Utc::now() + Duration::milliseconds(1500),
        browser_session_id: None,
        simulation: false,
    };
    let token: SecretString = "mock-token".into();
    assert!(api.button(&token, &event).await.is_err());
    event.browser_session_id = Some(Uuid::new_v4());
    event.simulation = true;
    assert!(api.button(&token, &event).await.is_err());
    event.simulation = false;
    event.expires_at = Utc::now() - Duration::milliseconds(1);
    assert!(api.button(&token, &event).await.is_err());
    assert!(server
        .received_requests()
        .await
        .expect("requests")
        .is_empty());
}
async fn read_until(
    serial: &mut impl SerialTransport,
    codec: &mut SerialProtocolCodec,
    predicate: impl Fn(&Frame) -> bool,
) -> Frame {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut bytes = [0; 512];
        loop {
            let n = serial.read(&mut bytes).await.expect("read");
            assert!(n > 0);
            for frame in codec.feed(&bytes[..n]) {
                let frame = frame.expect("valid frame");
                if predicate(&frame) {
                    return frame;
                }
            }
        }
    })
    .await
    .expect("response timeout")
}
#[tokio::test]
async fn backend_command_through_same_serial_protocol_and_result_api() {
    let server = MockServer::start().await;
    let id = Uuid::new_v4();
    let device = Uuid::new_v4();
    let token: SecretString = "mock-scoped-token".into();
    let command = RemoteCommand {
        contract: "hardware.v1".into(),
        command_id: id,
        device_id: device,
        action: "MOVE_TO".into(),
        target_station: 4,
        order_id: 42,
        issued_at: Utc::now() - Duration::seconds(1),
        expires_at: Utc::now() + Duration::seconds(30),
        lease_token: "fence-test".into(),
        attempt: 1,
        version: 1,
    };
    Mock::given(method("GET"))
        .and(path("/api/device/commands/next"))
        .and(query_param("waitSeconds", "20"))
        .and(header("Authorization", "Bearer mock-scoped-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&command))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/device/commands/{id}/ack")))
        .and(body_json(
            serde_json::json!({"contract":"hardware.v1","leaseToken":"fence-test","version":1}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/device/commands/{id}/events")))
        .and(header("X-Hardware-Contract", "hardware.v1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let api = client(&server);
    let received = api
        .next_command(&token)
        .await
        .expect("poll")
        .expect("command");
    received.validate(device, Utc::now()).expect("validate");
    api.ack(&token, &received).await.expect("fence ack");
    let (mut serial, _control, task) = simulator::connect();
    let mut codec = SerialProtocolCodec::default();
    let nonce = Uuid::new_v4().simple().to_string();
    serial
        .write(
            &Frame::Hello {
                v: 1,
                nonce: nonce.clone(),
            }
            .encode()
            .expect("encode"),
        )
        .await
        .expect("hello");
    assert!(
        matches!(read_until(&mut serial,&mut codec,|f|matches!(f,Frame::HelloAck{..})).await,Frame::HelloAck{nonce:n,..}if n==nonce)
    );
    serial
        .write(
            &Frame::MoveTo {
                v: 1,
                command_id: id,
                station: 4,
            }
            .encode()
            .expect("encode"),
        )
        .await
        .expect("move");
    let arrived = read_until(&mut serial, &mut codec, |f| {
        matches!(f, Frame::Arrived { .. })
    })
    .await;
    assert!(matches!(arrived,Frame::Arrived{command_id,station:4,..}if command_id==id));
    api.publish(&token, &CommandExecutionResult::new(id, "ARRIVED"))
        .await
        .expect("publish");
    drop(serial);
    assert!(task.await.expect("join").is_ok());
}
#[tokio::test]
async fn redirects_and_oversized_backend_responses_rejected() {
    let server = MockServer::start().await;
    let api = client(&server);
    let token: SecretString = "mock-token".into();
    Mock::given(path("/api/device/config"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "https://example.com"))
        .mount(&server)
        .await;
    assert!(matches!(
        api.config(&token, Uuid::new_v4()).await,
        Err(Error::Http(302))
    ));
    server.reset().await;
    Mock::given(path("/api/device/config"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(65537)))
        .mount(&server)
        .await;
    assert!(api.config(&token, Uuid::new_v4()).await.is_err());
}
