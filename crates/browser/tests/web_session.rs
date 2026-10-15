// SPDX-License-Identifier: Apache-2.0

//! Browser sessions against a local mock of the ChatGPT page, driving a real (headless) Edge
//! with a temporary profile (ExecPlan 017 §8). Skipped when Edge is not installed.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use loadout_browser::{
    AccountEvents, AccountSettings, ConversationMode, Launcher, WebAccount, find_edge,
};
use loadout_platform::OwnedJob;
use loadout_protocol::{CanonicalEvent, CanonicalRequest, Message, Role};
use loadout_provider_sdk::web::{GateState, SessionError, SessionRequest, SessionUpstream};
use loadout_provider_sdk::{CatalogModel, FailureKind, ProviderConfig, ProviderModule};
use loadout_providers::WebChatGpt;
use serde_json::Value;

#[path = "support/mock_chatgpt.rs"]
mod mock;

use mock::Mode;

#[derive(Debug, Default)]
struct Recorder {
    defaults: Mutex<Vec<String>>,
    attention: Mutex<Vec<String>>,
    models: Mutex<Vec<Vec<CatalogModel>>>,
}

impl AccountEvents for Recorder {
    fn default_conversation(&self, _: &str, url: &str) {
        self.defaults.lock().expect("lock").push(url.to_owned());
    }
    fn needs_attention(&self, _: &str, reason: &str) {
        self.attention.lock().expect("lock").push(reason.to_owned());
    }
    fn models(&self, _: &str, models: Vec<CatalogModel>) {
        self.models.lock().expect("lock").push(models);
    }
}

fn launcher() -> Option<Launcher> {
    let edge = match find_edge() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skipping: {e}");
            return None;
        }
    };
    Some(Launcher {
        edge: edge.path,
        job: Arc::new(OwnedJob::new().expect("job")),
        headless: true,
        login_headless: true,
    })
}

fn account(
    origin: &str,
    profile: &Path,
    settings: AccountSettings,
    events: Arc<Recorder>,
) -> Arc<WebAccount> {
    let module: Arc<dyn ProviderModule> = Arc::new(WebChatGpt);
    let config = ProviderConfig {
        slug: "web-chatgpt".into(),
        kind: "web-chatgpt".into(),
        base_url: Some(origin.to_owned()),
        settings: Value::Null,
    };
    module.validate(&config).expect("loopback origin");
    WebAccount::new(
        "acct-1",
        module,
        &config,
        profile.to_owned(),
        settings,
        events,
    )
    .expect("web account")
}

fn request(messages: &[(Role, &str)], instructions: &[&str]) -> CanonicalRequest {
    CanonicalRequest {
        model: "web-chatgpt/main/auto".into(),
        instructions: instructions.iter().map(|s| (*s).to_owned()).collect(),
        messages: messages
            .iter()
            .map(|(r, t)| Message::text(*r, *t))
            .collect(),
        stream: true,
        ..CanonicalRequest::default()
    }
}

async fn ask(
    account: &Arc<WebAccount>,
    req: CanonicalRequest,
    model: &str,
    conversation: Option<&str>,
) -> Result<String, SessionError> {
    let stream = account
        .clone()
        .execute(SessionRequest {
            request: req,
            model: model.to_owned(),
            conversation: conversation.map(str::to_owned),
        })
        .await?;
    let events: Vec<CanonicalEvent> = stream.collect().await;
    assert!(
        matches!(events.last(), Some(CanonicalEvent::Finish(_))),
        "answer ends with Finish: {events:?}"
    );
    Ok(events
        .iter()
        .filter_map(|e| match e {
            CanonicalEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect())
}

async fn wait_ready(account: &WebAccount) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while account.gate().state() != GateState::Ready {
        assert!(
            Instant::now() < deadline,
            "gate stays {:?}",
            account.gate().state()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn no_limits(mode: ConversationMode) -> AccountSettings {
    AccountSettings {
        mode,
        default_conversation: None,
        min_interval_secs: 0,
        daily_soft_cap: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conversations_are_typed_into_the_page_and_answers_read_back() {
    let Some(launcher) = launcher() else { return };
    let (origin, mock) = mock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let events = Arc::new(Recorder::default());
    let acct = account(
        &origin,
        &dir.path().join("profile"),
        no_limits(ConversationMode::Persistent),
        events.clone(),
    );

    acct.start(&launcher).await.expect("start");
    assert_eq!(acct.gate().state(), GateState::Ready);
    let listed = events.models.lock().expect("lock").clone();
    let ids: Vec<String> = listed
        .last()
        .expect("model list read while the page loaded")
        .iter()
        .map(|m| m.upstream_id.clone())
        .collect();
    assert_eq!(ids, ["auto", "gpt-5", "gpt-5-thinking"]);

    // 1. A new default conversation, with the system text as a marked prefix.
    let a = ask(
        &acct,
        request(&[(Role::User, "Review this:\nline two")], &["Be terse."]),
        "auto",
        None,
    )
    .await
    .expect("first answer");
    let got = mock.received();
    assert_eq!(got.len(), 1);
    assert!(
        got[0]
            .text
            .contains("[System instructions from the API caller]")
    );
    assert!(got[0].text.contains("Be terse."));
    assert!(
        got[0].text.contains("line two"),
        "newlines survive: {:?}",
        got[0].text
    );
    assert_eq!(
        a,
        format!("turn 1 of {}: {}", got[0].conversation, got[0].text)
    );
    assert!(!got[0].temporary);
    let conv1 = got[0].conversation.clone();
    wait_ready(&acct).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        events.defaults.lock().expect("lock").as_slice(),
        [format!("{origin}/c/{conv1}")],
        "the new conversation became the default"
    );

    // 2. The default conversation continues with only the newest user message.
    let b = ask(
        &acct,
        request(
            &[
                (Role::User, "Review this:\nline two"),
                (Role::Assistant, "ok"),
                (Role::User, "Next round"),
            ],
            &["Be terse."],
        ),
        "auto",
        None,
    )
    .await
    .expect("second answer");
    let got = mock.received();
    assert_eq!(got[1].text, "Next round");
    assert_eq!(got[1].conversation, conv1);
    assert_eq!(b, format!("turn 2 of {conv1}: Next round"));

    // 3. An explicit conversation key gets its own conversation, then keeps it.
    ask(
        &acct,
        request(&[(Role::User, "k1 first")], &[]),
        "auto",
        Some("key-1"),
    )
    .await
    .expect("k1");
    ask(
        &acct,
        request(&[(Role::User, "k1 second")], &[]),
        "gpt-5",
        Some("key-1"),
    )
    .await
    .expect("k1 again");
    let got = mock.received();
    assert_ne!(got[2].conversation, conv1);
    assert_eq!(got[3].conversation, got[2].conversation);
    assert_eq!(got[3].turn, 2);
    assert_eq!(
        got[3].model, "gpt-5",
        "the model was selected through the URL"
    );

    // 4. Stateless: a temporary conversation holding the whole history.
    acct.set_settings(no_limits(ConversationMode::Stateless));
    ask(
        &acct,
        request(
            &[
                (Role::User, "Q1"),
                (Role::Assistant, "A1"),
                (Role::User, "Q2"),
            ],
            &[],
        ),
        "auto",
        None,
    )
    .await
    .expect("stateless");
    let got = mock.received();
    assert!(got[4].temporary);
    // The mock reads the composer with `innerText`, which renders an empty paragraph as an
    // extra line break; compare without counting blank lines.
    let squash = |t: &str| {
        t.split('\n')
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        squash(&got[4].text),
        "[User]\nQ1\n[Assistant]\nA1\n[User]\nQ2"
    );

    // 5. Parameters the page cannot honour are refused before anything is typed.
    let mut sampled = request(&[(Role::User, "x")], &[]);
    sampled.temperature = Some(0.3);
    assert!(matches!(
        ask(&acct, sampled, "auto", None).await,
        Err(SessionError::Unsupported(_))
    ));
    assert_eq!(mock.received().len(), 5);

    acct.stop().await;
    assert_eq!(acct.gate().state(), GateState::Stopped);
    assert!(!acct.is_running());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn limits_cancellation_and_sign_out() {
    let Some(launcher) = launcher() else { return };
    let (origin, mock) = mock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let events = Arc::new(Recorder::default());
    let acct = account(
        &origin,
        &dir.path().join("profile"),
        no_limits(ConversationMode::Persistent),
        events.clone(),
    );
    acct.start(&launcher).await.expect("start");

    // A usage cap cools the session down until the reset the page reported.
    mock.set_mode(Mode::Capped);
    let before = loadout_router::now_ms();
    match ask(&acct, request(&[(Role::User, "hi")], &[]), "auto", None).await {
        Err(SessionError::Failed(f)) => assert_eq!(
            f.kind,
            FailureKind::QuotaExhausted {
                reset_after: Some(Duration::from_secs(1800))
            }
        ),
        other => panic!("expected a quota failure, got {other:?}"),
    }
    assert_eq!(acct.gate().state(), GateState::Cooling);
    assert!(acct.gate().next_allowed_ms() >= before + 1_790_000);
    assert!(acct.gate().unavailable_reason(before + 1000).is_some());

    // The client leaving mid-answer presses the page's stop control.
    acct.gate().set_state(GateState::Ready);
    mock.set_mode(Mode::Slow);
    let mut stream = acct
        .clone()
        .execute(SessionRequest {
            request: request(&[(Role::User, "a long answer please")], &[]),
            model: "auto".into(),
            conversation: None,
        })
        .await
        .expect("answer starts");
    let first = stream.next().await;
    assert!(first.is_some());
    drop(stream);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !mock.aborted.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "the page did not stop the answer"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    wait_ready(&acct).await;

    // Signed out: the request fails over and the session asks for the user.
    mock.set_mode(Mode::LoggedOut);
    match ask(
        &acct,
        request(&[(Role::User, "hi")], &[]),
        "auto",
        Some("fresh"),
    )
    .await
    {
        Err(SessionError::Failed(f)) => assert_eq!(f.kind, FailureKind::Auth),
        other => panic!("expected an auth failure, got {other:?}"),
    }
    assert_eq!(acct.gate().state(), GateState::NeedsAttention);
    assert!(
        events
            .attention
            .lock()
            .expect("lock")
            .iter()
            .any(|r| r.contains("signed out"))
    );
    assert!(acct.status().attention.is_some());

    // A verification page is reported, never acted on.
    mock.set_mode(Mode::Challenge);
    match ask(
        &acct,
        request(&[(Role::User, "hi")], &[]),
        "auto",
        Some("fresh-2"),
    )
    .await
    {
        Err(SessionError::Failed(f)) => assert_eq!(f.kind, FailureKind::Auth),
        other => panic!("expected an auth failure, got {other:?}"),
    }
    assert!(
        events
            .attention
            .lock()
            .expect("lock")
            .iter()
            .any(|r| r.contains("verification"))
    );
    acct.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_in_uses_an_uncontrolled_window_then_resumes() {
    let Some(launcher) = launcher() else { return };
    let (origin, mock) = mock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let acct = account(
        &origin,
        &dir.path().join("profile"),
        no_limits(ConversationMode::Persistent),
        Arc::new(Recorder::default()),
    );
    acct.start(&launcher).await.expect("start");
    assert!(acct.is_running());

    // Sign-in: the controlled browser closes, a plain window takes the profile.
    acct.begin_login(&launcher).await.expect("sign-in window");
    assert!(acct.signing_in());
    assert!(!acct.is_running(), "no controlled browser while signing in");
    assert_eq!(acct.gate().state(), GateState::SigningIn);
    assert_eq!(
        acct.gate().unavailable_reason(1),
        Some("web session signing in")
    );
    assert!(matches!(
        acct.start(&launcher).await,
        Err(loadout_browser::BrowserError::SigningIn)
    ));
    assert!(acct.check().await.is_none());
    assert_eq!(
        acct.gate().state(),
        GateState::SigningIn,
        "checks leave it alone"
    );
    assert!(matches!(
        ask(&acct, request(&[(Role::User, "hi")], &[]), "auto", None).await,
        Err(SessionError::Busy(_))
    ));
    // The plain window loads the site like any browser, without Loadout reading it.
    let deadline = Instant::now() + Duration::from_secs(15);
    while mock.model_list_requests.load(Ordering::Acquire) < 2 {
        assert!(
            Instant::now() < deadline,
            "sign-in window did not load the page"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Done: the window closes and the controlled browser starts again on the same profile.
    acct.finish_login().await;
    assert!(!acct.signing_in());
    assert_eq!(acct.gate().state(), GateState::Stopped);
    acct.start(&launcher).await.expect("restart after sign-in");
    assert_eq!(acct.gate().state(), GateState::Ready);
    acct.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_the_job_ends_the_browser() {
    let Some(launcher) = launcher() else { return };
    let (origin, _mock) = mock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let acct = account(
        &origin,
        &dir.path().join("profile"),
        no_limits(ConversationMode::Persistent),
        Arc::new(Recorder::default()),
    );
    acct.start(&launcher).await.expect("start");
    assert!(acct.is_running());
    drop(launcher);
    // Only this test's job holds the browser: dropping the last handle ends it.
    let deadline = Instant::now() + Duration::from_secs(15);
    while acct.is_running() {
        assert!(Instant::now() < deadline, "browser still running");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(acct.check().await.is_none());
    assert_eq!(acct.gate().state(), GateState::Stopped);
}
