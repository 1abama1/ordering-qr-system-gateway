use gateway_core::{
    backend::OsCredentialStore,
    domain::GatewayStatus,
    runtime::UiAction,
    serial::{self, DiscoveredPort},
    GatewayHandle, GatewaySupervisor,
};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tauri_plugin_autostart::ManagerExt;

pub mod commands {
    use super::*;
    #[tauri::command]
    pub fn gateway_status(handle: State<'_, GatewayHandle>) -> GatewayStatus {
        handle.snapshot()
    }
    #[tauri::command]
    pub async fn gateway_action(
        handle: State<'_, GatewayHandle>,
        action: UiAction,
    ) -> Result<(), String> {
        if matches!(action, UiAction::Shutdown {}) {
            return Err("Use explicit Quit action".into());
        }
        handle.execute(action).await.map_err(|e| e.to_string())
    }
}
#[tauri::command]
async fn serial_ports() -> Result<Vec<DiscoveredPort>, String> {
    tokio::task::spawn_blocking(serial::discover)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn export_diagnostics(handle: State<'_, GatewayHandle>) -> Result<String, String> {
    handle.diagnostics().map_err(|e| e.to_string())
}
#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}
#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}
async fn quit(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.state::<GatewayHandle>().inner().clone();
    handle
        .execute(UiAction::Shutdown {})
        .await
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}
#[tauri::command]
async fn quit_gateway(app: tauri::AppHandle) -> Result<(), String> {
    quit(app).await
}
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus())
        {
            tracing::warn!(error=%e,"cannot focus dashboard");
        }
    }
}
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "gateway_core=info,dastor_gateway=info",
        ))
        .with_ansi(false)
        .init();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::gateway_status,
            commands::gateway_action,
            serial_ports,
            export_diagnostics,
            get_autostart,
            set_autostart,
            quit_gateway
        ])
        .setup(|app| {
            let directory = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let (handle, task) = tauri::async_runtime::block_on(async {
                GatewaySupervisor::start(directory.join("gateway.db"), Arc::new(OsCredentialStore))
            })?;
            let mut status = handle.status.clone();
            app.manage(handle);
            let open =
                MenuItem::with_id(app, "open", "Открыть DASTOR Gateway", true, None::<&str>)?;
            let connection =
                MenuItem::with_id(app, "connection", "Backend: offline", false, None::<&str>)?;
            let pause =
                MenuItem::with_id(app, "pause", "Возобновить приём команд", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "quit", "Завершить агент", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &connection, &pause, &exit])?;
            let mut builder = TrayIconBuilder::with_id("gateway-tray")
                .menu(&menu)
                .tooltip("DASTOR Gateway · offline")
                .show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            }
            builder
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "pause" => {
                        let handle = app.state::<GatewayHandle>().inner().clone();
                        tauri::async_runtime::spawn(async move {
                            let paused = !handle.snapshot().paused;
                            if let Err(e) = handle.execute(UiAction::Pause { paused }).await {
                                tracing::warn!(error=%e,"tray pause rejected");
                            }
                        });
                    }
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = quit(app).await {
                                tracing::error!(error=%e,"graceful quit failed");
                            }
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while status.changed().await.is_ok() {
                    let snapshot = status.borrow().clone();
                    if let Err(e) = app_handle.emit("gateway:status", &snapshot) {
                        tracing::warn!(error=%e,"event bridge failed");
                    }
                    if let Err(e) = connection.set_text(if snapshot.backend_connected {
                        "Backend: connected"
                    } else {
                        "Backend: offline"
                    }) {
                        tracing::warn!(error=%e,"tray status failed");
                    }
                    if let Err(e) = pause.set_text(if snapshot.paused {
                        "Возобновить приём команд"
                    } else {
                        "Приостановить приём команд"
                    }) {
                        tracing::warn!(error=%e,"tray text failed");
                    }
                    if let Some(tray) = app_handle.tray_by_id("gateway-tray") {
                        if let Err(e) = tray.set_tooltip(Some(format!(
                            "DASTOR · {:?} · {}",
                            snapshot.state,
                            if snapshot.simulation {
                                "SIMULATION"
                            } else if snapshot.backend_connected {
                                "online"
                            } else {
                                "offline"
                            }
                        ))) {
                            tracing::warn!(error=%e,"tray tooltip failed");
                        }
                    }
                }
            });
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match task.await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::error!(error=%e,"supervisor stopped");
                        if let Err(error) = app_handle.emit("gateway:fatal", e.to_string()) {
                            tracing::error!(%error,"fatal event failed");
                        }
                    }
                    Err(e) => tracing::error!(error=%e,"supervisor panicked"),
                }
            });
            if std::env::args().any(|a| a == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                if app
                    .state::<GatewayHandle>()
                    .snapshot()
                    .settings
                    .minimize_to_tray
                {
                    if let Err(e) = window.hide() {
                        tracing::warn!(error=%e,"window hide failed");
                    }
                } else {
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = quit(app).await {
                            tracing::error!(error=%e,"quit failed");
                        }
                    });
                }
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!(error=%e,"application startup failed");
    }
}
