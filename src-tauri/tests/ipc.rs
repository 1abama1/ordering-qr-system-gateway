use dastor_gateway::commands;
use gateway_core::{
    backend::{CredentialStore, DeviceCredentials},
    domain::{Error, Result},
};
use gateway_core::{runtime::UiAction, GatewaySupervisor};
use std::sync::Arc;
struct Vault;
impl CredentialStore for Vault {
    fn load(&self, _: &str) -> Result<Option<DeviceCredentials>> {
        Ok(None)
    }
    fn save(&self, _: &DeviceCredentials) -> Result<()> {
        Err(Error::Credentials)
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn typed_ipc_status_and_invalid_actions() {
    let temp = tempfile::tempdir().expect("temp");
    let (handle, task) =
        GatewaySupervisor::start(temp.path().join("test.db"), Arc::new(Vault)).expect("start");
    let app = tauri::test::mock_builder()
        .manage(handle.clone())
        .invoke_handler(tauri::generate_handler![
            commands::gateway_status,
            commands::gateway_action
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window");
    let request = |cmd: &str, body: serde_json::Value| tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://tauri.localhost".parse().expect("url"),
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    };
    let snapshot =
        tauri::test::get_ipc_response(&window, request("gateway_status", serde_json::json!({})))
            .expect("IPC status")
            .deserialize::<serde_json::Value>()
            .expect("JSON");
    assert_eq!(snapshot["backendConnected"], false);
    assert!(snapshot.get("accessToken").is_none());
    assert!(snapshot.get("deviceSecret").is_none());
    assert!(tauri::test::get_ipc_response(
        &window,
        request(
            "gateway_action",
            serde_json::json!({"action":{"type":"serial_write","bytes":"MOVE_TO"}})
        )
    )
    .is_err());
    assert!(tauri::test::get_ipc_response(
        &window,
        request(
            "gateway_action",
            serde_json::json!({"action":{"type":"shutdown"}})
        )
    )
    .is_err());
    handle
        .execute(UiAction::Shutdown {})
        .await
        .expect("shutdown");
    assert!(task.await.expect("join").is_ok());
}
