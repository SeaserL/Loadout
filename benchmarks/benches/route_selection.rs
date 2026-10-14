// SPDX-License-Identifier: Apache-2.0

//! Name resolution + eligibility over a 100-target table (target P95 < 2 ms), including a
//! virtual model over every target with session affinity (ExecPlan 004).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use loadout_protocol::Protocol;
use loadout_protocol::hints::RouteHints;
use loadout_provider_sdk::web::{
    GateState, SessionError, SessionGate, SessionRequest, SessionStream, SessionUpstream,
    WebBinding,
};
use loadout_provider_sdk::{
    BoxFuture, Capabilities, Endpoint, ProviderConfig, ProviderModule, TrustLevel,
};
use loadout_providers::OpenAiCompatible;
use loadout_router::{
    HealthRegistry, Prices, RouteTable, RouteTarget, SelectRequest, SessionAffinity, TargetId,
    select,
};
use serde_json::Value;

/// Never executed: selection only reads the gate.
struct NoSession;

impl SessionUpstream for NoSession {
    fn execute(
        self: Arc<Self>,
        _request: SessionRequest,
    ) -> BoxFuture<'static, Result<SessionStream, SessionError>> {
        Box::pin(async { Err(SessionError::Busy("benchmark")) })
    }
}

fn table(reg: &HealthRegistry) -> RouteTable {
    table_with(reg, None)
}

/// `web`: every target is a browser session sharing one ready gate (ExecPlan 017 §11).
fn table_with(reg: &HealthRegistry, web: Option<Arc<WebBinding>>) -> RouteTable {
    let module: Arc<dyn ProviderModule> = Arc::new(OpenAiCompatible);
    let config = Arc::new(ProviderConfig {
        slug: "p".into(),
        kind: "openai-compatible".into(),
        base_url: Some("https://x.test/v1".into()),
        settings: Value::Null,
    });
    let mut targets = Vec::new();
    for account in 0..10 {
        for model in 0..5 {
            for protocol in [Protocol::Chat, Protocol::Messages] {
                let id: TargetId = Arc::from(format!("a{account}-m{model}:{}", protocol.as_str()));
                targets.push(Arc::new(RouteTarget {
                    health: reg.get_or_create(&id),
                    id,
                    provider_slug: Arc::from(format!("p{}", account % 3).as_str()),
                    account_id: Arc::from(format!("a{account}").as_str()),
                    account_alias: Arc::from(format!("acct{account}").as_str()),
                    account_position: account,
                    upstream_model: Arc::from(format!("model-{model}").as_str()),
                    protocol,
                    trust: TrustLevel::OfficialApi,
                    opted_in: false,
                    context_window: Some(128_000),
                    max_output_tokens: None,
                    capabilities: Capabilities::DEFAULT_MODEL,
                    prices: Prices::default(),
                    endpoint: Endpoint::new("https://x.test/v1/chat/completions".into()),
                    web: web.clone(),
                    module: module.clone(),
                    config: config.clone(),
                }));
            }
        }
    }
    let ids: Vec<TargetId> = targets.iter().map(|t| t.id.clone()).collect();
    RouteTable::new(1, targets).with_virtual("default", &ids)
}

fn route_selection(c: &mut Criterion) {
    let reg = HealthRegistry::default();
    let t = table(&reg);
    assert_eq!(t.targets().len(), 100);
    let hints = RouteHints {
        model: "p1/model-3".into(),
        stream: true,
        has_tools: true,
        has_images: false,
        wants_reasoning: false,
        previous_response_id: None,
        has_conversation: false,
        estimated_input_tokens: 2_000,
        client_wants_usage: false,
    };
    c.bench_function("route_selection/provider_model_100_targets", |b| {
        b.iter(|| {
            let res = t.resolve(black_box(&hints.model)).expect("resolve");
            select(
                &res,
                &SelectRequest {
                    ingress: Protocol::Chat,
                    hints: &hints,
                    now_ms: 0,
                    max_attempts: 3,
                    pinned_target: None,
                    allow_account_failover: true,
                    preferred: None,
                },
            )
        })
    });
    let sessions = SessionAffinity::default();
    for i in 0..5_000 {
        sessions.put(&format!("session-{i}"), Arc::from("a7-m4"), 0);
    }
    let mut virtual_hints = hints.clone();
    virtual_hints.model = "loadout/default".into();
    c.bench_function("route_selection/virtual_100_targets_with_session", |b| {
        b.iter(|| {
            let preferred = sessions.get(black_box("session-4242"), 1);
            let res = t.resolve(&virtual_hints.model).expect("resolve");
            select(
                &res,
                &SelectRequest {
                    ingress: Protocol::Chat,
                    hints: &virtual_hints,
                    now_ms: 0,
                    max_attempts: 3,
                    pinned_target: None,
                    allow_account_failover: true,
                    preferred: preferred.as_deref(),
                },
            )
        })
    });
    c.bench_function("route_selection/exact_100_targets", |b| {
        b.iter(|| {
            t.resolve(black_box("p1/acct4/model-2"))
                .expect("resolve")
                .candidates
                .len()
        })
    });

    let gate = Arc::new(SessionGate::default());
    gate.set_state(GateState::Ready);
    let web = Arc::new(WebBinding {
        gate,
        upstream: Arc::new(NoSession),
    });
    let web_reg = HealthRegistry::default();
    let web_table = table_with(&web_reg, Some(web));
    let mut text_hints = hints.clone();
    text_hints.has_tools = false;
    c.bench_function("route_selection/provider_model_100_web_gates", |b| {
        b.iter(|| {
            let res = web_table
                .resolve(black_box(&text_hints.model))
                .expect("resolve");
            select(
                &res,
                &SelectRequest {
                    ingress: Protocol::Chat,
                    hints: &text_hints,
                    now_ms: 1,
                    max_attempts: 3,
                    pinned_target: None,
                    allow_account_failover: true,
                    preferred: None,
                },
            )
        })
    });
}

criterion_group!(benches, route_selection);
criterion_main!(benches);
