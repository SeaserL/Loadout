// SPDX-License-Identifier: Apache-2.0

//! Daemon notifications as Windows toasts (ExecPlan 012, demo). A background task keeps an
//! event subscription open on the daemon's pipe and shows each `notify` event; the tray
//! tooltip carries the latest one. Works while the window is hidden.

use std::sync::Mutex;
use std::time::Duration;

use loadout_ipc::api::{NOTIFY_EVENT, Notification};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::bridge::Bridge;

/// Delay before re-subscribing after the daemon went away or was not running.
const RECONNECT: Duration = Duration::from_secs(5);

/// The UI language, set by the web UI through `set_tray_language`.
pub struct UiLanguage(pub Mutex<String>);

impl Default for UiLanguage {
    fn default() -> Self {
        Self(Mutex::new("zh-CN".to_owned()))
    }
}

/// Toast title and body in the UI language. English uses the daemon's own text.
pub fn toast_text(n: &Notification, lang: &str) -> (String, String) {
    if lang == "en" {
        return (n.title.clone(), n.body.clone());
    }
    let s = &n.subject;
    let num = n.value.unwrap_or(0.0);
    match n.kind.as_str() {
        "quota_low" => (
            "额度即将用完".into(),
            format!(
                "{s}：{} 剩余 {num:.0}%",
                n.detail.as_deref().unwrap_or("额度窗口")
            ),
        ),
        "balance_low" => ("余额不足".into(), format!("{s}：余额仅剩 ${num:.2}")),
        "auth_failed" => (
            "凭证被拒绝".into(),
            format!("{s}：上游拒绝了 API key 或登录（401/403），请更新 key 或重新登录。"),
        ),
        "login_expiring" if num <= 0.0 => (
            "登录即将过期".into(),
            format!("{s}：登录已过期或即将过期，请重新登录。"),
        ),
        "login_expiring" => (
            "登录即将过期".into(),
            format!("{s}：登录将在 {num:.0} 小时后过期且未能自动续期，请重新登录。"),
        ),
        "client_cost_limit" => (
            "已达每日花费上限".into(),
            format!("客户端 {s} 已达每日花费上限 ${num:.2}，UTC 零点前的请求将被拒绝。"),
        ),
        _ => (n.title.clone(), n.body.clone()),
    }
}

fn show(app: &AppHandle, n: &Notification) {
    let lang = app
        .state::<UiLanguage>()
        .0
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default();
    let (title, body) = toast_text(n, &lang);
    if let Err(e) = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        eprintln!("loadout-app: cannot show notification: {e}");
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("Loadout — {title}")));
    }
}

/// Keeps an event subscription to the daemon and shows its notifications. Runs until the
/// app exits.
pub async fn listen(app: AppHandle) {
    let bridge = app.state::<Bridge>().inner().clone();
    loop {
        if let Ok(mut client) = bridge.subscribe().await {
            while let Ok(Some(event)) = client.next_event().await {
                if event.name != NOTIFY_EVENT {
                    continue;
                }
                match serde_json::from_value::<Notification>(event.data) {
                    Ok(n) => show(&app, &n),
                    Err(e) => eprintln!("loadout-app: malformed notification: {e}"),
                }
            }
        }
        tokio::time::sleep(RECONNECT).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(kind: &str, value: Option<f64>) -> Notification {
        Notification {
            kind: kind.into(),
            key: "openai/main".into(),
            title: "English title".into(),
            body: "English body".into(),
            subject: "openai/main".into(),
            detail: Some("5h".into()),
            value,
        }
    }

    #[test]
    fn localizes_known_kinds() {
        let (title, body) = toast_text(&n("quota_low", Some(7.4)), "zh-CN");
        assert_eq!(title, "额度即将用完");
        assert_eq!(body, "openai/main：5h 剩余 7%");
        let (_, body) = toast_text(&n("login_expiring", Some(0.0)), "zh-CN");
        assert!(body.contains("已过期"));
        assert_eq!(
            toast_text(&n("quota_low", Some(7.0)), "en"),
            ("English title".into(), "English body".into())
        );
        assert_eq!(
            toast_text(&n("from_the_future", None), "zh-CN").0,
            "English title"
        );
    }
}
