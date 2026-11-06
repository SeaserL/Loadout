// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Window and tray for Loadout Fusion. The backend is the packaged OmniRoute runtime, owned by its
//! own supervisor: closing or quitting this window leaves it serving clients (ADR-002). There is no
//! login page: a helper reads the machine secret from the OS vault and hands the session cookie to
//! this process over a private pipe, so it never reaches a file, a command line or the page's JS.
//!
//! `--background` is the sign-in entry (ADR-002): it starts the backend and waits in the tray. The
//! window, and with it the WebView, is only built when someone opens it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::Cookie;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

const WINDOW: &str = "main";
const TRAY: &str = "main";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PRODUCTION_PORT: u16 = 8317;

#[derive(Deserialize)]
struct Prepared {
    url: String,
    session: String,
}

/// What the window needs, kept so that it can be built later than the tray.
struct Launch {
    root: PathBuf,
    instance: Option<String>,
    profile: PathBuf,
}

fn package_root() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "package folder not found".to_owned())
}

/// The data directory: `--instance <dir>`, else the default beside the package.
fn instance_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--instance" {
            return args.next();
        }
    }
    None
}

fn background_requested(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == "--background")
}

fn default_instance(root: &Path) -> PathBuf {
    root.parent()
        .unwrap_or(root)
        .join("loadout-fusion-isolated")
        .join("desktop-current")
}

fn helper(root: &Path, command: &str, instance: Option<&str>) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new(root.join("node.exe"));
    c.arg(root.join("tools").join("desktop.mjs")).arg(command);
    if let Some(dir) = instance {
        c.arg(dir);
    }
    c.current_dir(root)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    c
}

fn prepare(root: &Path, instance: Option<&str>) -> Result<Prepared, String> {
    let output = helper(root, "prepare-session", instance)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "后台返回了无法识别的内容".to_owned())
}

fn is_local_console(url: &tauri::Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && matches!(url.port(), Some(p) if p != PRODUCTION_PORT && p > 1024)
        && url.path() == "/loadout/index.html"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

/// Shows the window, building it first when this process started in the background.
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    if let Err(e) = open_window(app) {
        tray_note(app, &format!("打不开窗口：{e}"));
    }
}

fn open_window(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let launch = app.state::<Launch>();
    std::fs::create_dir_all(&launch.profile)?;
    WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html".into()))
        .title("Loadout")
        .inner_size(1320.0, 880.0)
        .min_inner_size(980.0, 640.0)
        .center()
        .data_directory(launch.profile.clone())
        .on_navigation(|target| {
            // The console and the bundled splash; everything else stays out of the window.
            (target.scheme() == "http"
                && target.host_str() == Some("127.0.0.1")
                && target.path().starts_with("/loadout/"))
                || target.host_str() == Some("tauri.localhost")
        })
        .build()?;
    connect(app.clone(), launch.root.clone(), launch.instance.clone());
    Ok(())
}

fn report(app: &tauri::AppHandle, message: &str) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let text = serde_json::to_string(message).unwrap_or_else(|_| "\"启动失败\"".to_owned());
        let _ = window.eval(format!(
            "document.getElementById('status').textContent={text};document.getElementById('bar').hidden=true;"
        ));
    }
}

/// With no window, the tray is the only place a problem can show.
fn tray_note(app: &tauri::AppHandle, message: &str) {
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_tooltip(Some(format!("Loadout：{message}")));
    }
}

/// Starts or reuses the backend, then points the window at it with the session already in place.
fn connect(app: tauri::AppHandle, root: PathBuf, instance: Option<String>) {
    std::thread::spawn(move || {
        let prepared = match prepare(&root, instance.as_deref()) {
            Ok(p) => p,
            Err(message) => return report(&app, &format!("启动失败：{message}")),
        };
        let Ok(url) = prepared.url.parse::<tauri::Url>() else {
            return report(&app, "启动失败：后台地址无效");
        };
        if !is_local_console(&url) {
            return report(&app, "启动失败：后台地址不在本机");
        }
        let Some((name, value)) = prepared.session.split_once('=') else {
            return report(&app, "启动失败：没有拿到会话");
        };
        let Some(window) = app.get_webview_window(WINDOW) else {
            return;
        };
        let cookie = Cookie::build((name.to_owned(), value.to_owned()))
            .domain("127.0.0.1")
            .path("/")
            .http_only(true)
            .same_site(tauri::webview::cookie::SameSite::Lax)
            .build();
        if let Err(e) = window.set_cookie(cookie) {
            return report(&app, &format!("启动失败：{e}"));
        }
        if let Err(e) = window.navigate(url) {
            report(&app, &format!("启动失败：{e}"));
        }
    });
}

/// The sign-in entry: the gateway comes up and nothing appears on screen.
fn start_in_background(app: tauri::AppHandle, root: PathBuf, instance: Option<String>) {
    std::thread::spawn(move || {
        let output = helper(&root, "prepare", instance.as_deref())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => {}
            Ok(o) => tray_note(
                &app,
                &format!("启动失败：{}", String::from_utf8_lossy(&o.stderr).trim()),
            ),
            Err(e) => tray_note(&app, &format!("启动失败：{e}")),
        }
    });
}

fn stop_backend(root: &Path, instance: Option<&str>) {
    let _ = helper(root, "stop", instance)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = package_root()?;
    let instance = instance_arg();
    let background = background_requested(std::env::args().skip(1));
    let profile = instance
        .clone()
        .map_or_else(|| default_instance(&root), PathBuf::from)
        .join("webview");
    let quit_root = root.clone();
    let quit_instance = instance.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .setup(move |app| {
            app.manage(Launch {
                root: root.clone(),
                instance: instance.clone(),
                profile,
            });
            let open = MenuItem::with_id(app, "open", "打开 Loadout", true, None::<&str>)?;
            let leave =
                MenuItem::with_id(app, "leave", "退出窗口（网关继续运行）", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出并停止网关", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &leave, &quit])?;
            let mut tray = TrayIconBuilder::with_id(TRAY)
                .tooltip("Loadout")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "leave" => app.exit(0),
                    "quit" => {
                        stop_backend(&quit_root, quit_instance.as_deref());
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if background {
                start_in_background(app.handle().clone(), root.clone(), instance.clone());
            } else {
                open_window(app.handle())?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|_app, event| {
            // Closing the window keeps the tray and the gateway, as the original app does, but the
            // window really closes: its WebView holds about 350 MB, and the tray builds a new one
            // on demand. Only the tray's own exits end the process.
            if let RunEvent::ExitRequested { api, code, .. } = event
                && !exit_allowed(code)
            {
                api.prevent_exit();
            }
        });
    Ok(())
}

/// The last window closing asks to exit with no code; the tray's exits pass one.
fn exit_allowed(code: Option<i32>) -> bool {
    code.is_some()
}

#[cfg(test)]
mod tests {
    use super::{background_requested, exit_allowed, is_local_console};

    #[test]
    fn closing_the_window_does_not_exit() {
        assert!(!exit_allowed(None));
        assert!(exit_allowed(Some(0)));
    }

    #[test]
    fn config_is_separate_from_the_installed_app() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert_eq!(context.config().identifier, "dev.loadout.fusion");
        assert!(context.config().app.windows.is_empty());
        assert!(context.config().app.security.capabilities.is_empty());
    }

    #[test]
    fn background_is_an_explicit_flag() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(background_requested(args(&[
            "--instance",
            r"D:\x",
            "--background"
        ])));
        assert!(!background_requested(args(&["--instance", r"D:\x"])));
        assert!(!background_requested(args(&["--backgrounds"])));
    }

    #[test]
    fn only_the_local_console_is_accepted() {
        let parse = |target: &str| target.parse::<tauri::Url>().expect("test URL");
        assert!(is_local_console(&parse(
            "http://127.0.0.1:8318/loadout/index.html"
        )));
        for target in [
            "http://127.0.0.1:8317/loadout/index.html",
            "https://example.com/loadout/index.html",
            "http://0.0.0.0:8318/loadout/index.html",
            "http://127.0.0.1:8318/v1/chat/completions",
            "http://user:secret@127.0.0.1:8318/loadout/index.html",
            "http://127.0.0.1:8318/loadout/index.html?token=secret",
            "http://127.0.0.1:8318/loadout/index.html#secret",
        ] {
            assert!(!is_local_console(&parse(target)), "{target}");
        }
    }
}
