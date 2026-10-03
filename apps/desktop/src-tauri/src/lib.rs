// SPDX-License-Identifier: Apache-2.0

//! Loadout desktop app (ExecPlan 005): tray icon and management UI. A pure IPC client of
//! `loadoutd`; closing the window hides it, and quitting leaves the gateway running
//! (ADR-002).

mod bridge;

use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::Color;
use tauri::{AppHandle, Manager, State, Theme, WebviewWindow, WindowEvent};

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

/// Tray menu labels per UI language (the web UI picks the language; Chinese by default).
fn tray_labels(lang: &str) -> [&'static str; 4] {
    match lang {
        "en" => [
            "Open Loadout",
            "Start gateway",
            "Stop gateway",
            "Quit (the gateway keeps running)",
        ],
        _ => [
            "打开 Loadout",
            "启动网关",
            "停止网关",
            "退出（网关继续运行）",
        ],
    }
}

/// Tray menu items whose text follows the UI language.
struct TrayItems([MenuItem<tauri::Wry>; 4]);

/// Called by the web UI at startup and whenever the user switches language.
#[tauri::command]
fn set_tray_language(items: State<'_, TrayItems>, lang: String) -> Result<(), UiError> {
    for (item, label) in items.0.iter().zip(tray_labels(&lang)) {
        item.set_text(label).map_err(|e| UiError {
            code: "tray".into(),
            message: e.to_string(),
        })?;
    }
    Ok(())
}

/// Window background per theme; matches `--bg` in the web UI's styles.css.
fn theme_background(dark: bool) -> Color {
    if dark {
        Color(11, 13, 17, 255)
    } else {
        Color(246, 247, 245, 255)
    }
}

/// Called by the web UI when the theme changes, so the native window background (shown
/// before the page paints and while resizing) and the title bar match the page.
/// `preference` is `light`, `dark` or `system`; `dark` is the theme actually shown.
#[tauri::command]
fn set_window_theme(window: WebviewWindow, preference: String, dark: bool) -> Result<(), UiError> {
    let to_ui = |e: tauri::Error| UiError {
        code: "window".into(),
        message: e.to_string(),
    };
    let theme = match preference.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    };
    window.set_theme(theme).map_err(to_ui)?;
    window
        .set_background_color(Some(theme_background(dark)))
        .map_err(to_ui)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let [open_label, start_label, stop_label, quit_label] = tray_labels("zh-CN");
    let open = MenuItem::with_id(app, "open", open_label, true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", start_label, true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", stop_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    app.manage(TrayItems([
        open.clone(),
        start.clone(),
        stop.clone(),
        quit.clone(),
    ]));
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
            autostart_set,
            set_tray_language,
            set_window_theme
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            // Paint the window in the system theme until the page applies the user's choice.
            if let Some(w) = app.get_webview_window("main") {
                let dark = matches!(w.theme(), Ok(Theme::Dark));
                let _ = w.set_background_color(Some(theme_background(dark)));
            }
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
