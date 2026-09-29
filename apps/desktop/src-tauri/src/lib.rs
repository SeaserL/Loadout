// SPDX-License-Identifier: Apache-2.0

//! Loadout desktop app (ExecPlan 005): tray icon and management UI. A pure IPC client of
//! `loadoutd`; closing the window hides it, and quitting leaves the gateway running
//! (ADR-002).

mod bridge;

use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};

pub use bridge::{ALLOWED_METHODS, Bridge, UiError};

#[tauri::command]
async fn ipc_call(
    bridge: State<'_, Bridge>,
    method: String,
    params: Option<Value>,
) -> Result<Value, UiError> {
    bridge.call(&method, params.unwrap_or(Value::Null)).await
}

#[tauri::command]
async fn daemon_start(bridge: State<'_, Bridge>) -> Result<Value, UiError> {
    bridge.start_daemon().await
}

#[tauri::command]
async fn daemon_stop(bridge: State<'_, Bridge>) -> Result<(), UiError> {
    bridge.stop_daemon().await
}

/// Whether `loadoutd` starts at sign-in (HKCU Run).
#[tauri::command]
fn autostart_get() -> Result<bool, UiError> {
    match loadout_platform::Autostart::new().status() {
        Ok(loadout_platform::AutostartStatus::Disabled) => Ok(false),
        Ok(_) => Ok(true),
        Err(e) => Err(UiError {
            code: "platform".into(),
            message: e.to_string(),
        }),
    }
}

#[tauri::command]
fn autostart_set(enabled: bool) -> Result<bool, UiError> {
    let platform = |e: loadout_platform::PlatformError| UiError {
        code: "platform".into(),
        message: e.to_string(),
    };
    let autostart = loadout_platform::Autostart::new();
    if enabled {
        let exe = bridge::daemon_exe().map_err(|m| UiError {
            code: "not_installed".into(),
            message: m,
        })?;
        autostart
            .enable(&exe, &["--background"])
            .map_err(platform)?;
    } else {
        autostart.disable().map_err(platform)?;
    }
    Ok(enabled)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Loadout", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "Start gateway", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop gateway", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        "Quit (the gateway keeps running)",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &start,
            &stop,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Loadout")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "start" | "stop" => {
                let app = app.clone();
                let start = event.id.as_ref() == "start";
                tauri::async_runtime::spawn(async move {
                    let bridge = app.state::<Bridge>();
                    let result = if start {
                        bridge.start_daemon().await.map(|_| ())
                    } else {
                        bridge.stop_daemon().await
                    };
                    if let Err(e) = result {
                        eprintln!("loadout-app: {}", e.message);
                    }
                    // Let the UI refresh its status right away.
                    use tauri::Emitter;
                    let _ = app.emit("daemon-changed", ());
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Runs the desktop app until the user quits from the tray.
pub fn run() {
    let bridge = match Bridge::for_current_user() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("loadout-app: {e}");
            std::process::exit(1);
        }
    };
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .manage(bridge)
        .invoke_handler(tauri::generate_handler![
            ipc_call,
            daemon_start,
            daemon_stop,
            autostart_get,
            autostart_set
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            // ADR-002: the app starts the daemon when it is not running.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let bridge = handle.state::<Bridge>();
                if !bridge.is_running().await
                    && let Err(e) = bridge.start_daemon().await
                {
                    eprintln!("loadout-app: {}", e.message);
                }
                use tauri::Emitter;
                let _ = handle.emit("daemon-changed", ());
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window only hides it; the app lives in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("loadout-app: {e}");
        std::process::exit(1);
    }
}
