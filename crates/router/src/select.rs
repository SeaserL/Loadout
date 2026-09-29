// SPDX-License-Identifier: Apache-2.0

//! Candidate selection (ADR-003 §5–6): eligibility filters over the resolved chain, keeping
//! the chain's order (sticky: the first eligible target always wins), preferring the
//! ingress protocol within each account model (ADR-006 §2).

use std::sync::Arc;

use loadout_protocol::Protocol;
use loadout_protocol::hints::RouteHints;

use crate::{Resolution, ResolutionKind, RouteTarget, TargetId};

#[derive(Debug)]
pub struct SelectRequest<'a> {
    pub ingress: Protocol,
    pub hints: &'a RouteHints,
    pub now_ms: i64,
    pub max_attempts: usize,
    /// For stateful Responses requests: the target that created the referenced response.
    pub pinned_target: Option<&'a str>,
    /// Exact-model requests may fail over to the same model on other accounts of the same
    /// provider (ADR-003 §8); `false` restricts to the named account.
    pub allow_account_failover: bool,
}

#[derive(Debug, Default)]
pub struct Candidates {
    /// Ordered attempts.
    pub attempts: Vec<Arc<RouteTarget>>,
    /// Targets excluded, with the reason (for errors and debug logs).
    pub skipped: Vec<(TargetId, &'static str)>,
}

fn account_model_key(t: &RouteTarget) -> &str {
    t.id.rsplit_once(':').map_or(&*t.id, |(k, _)| k)
}

pub fn select(resolution: &Resolution<'_>, req: &SelectRequest<'_>) -> Candidates {
    let mut out = Candidates::default();
    let stateful = req.hints.is_stateful();

    // Group protocol variants of the same account model, keeping chain order.
    let mut groups: Vec<Vec<&Arc<RouteTarget>>> = Vec::new();
    for t in &resolution.candidates {
        match groups
            .iter_mut()
            .find(|g| account_model_key(g[0]) == account_model_key(t))
        {
            Some(g) => g.push(t),
            None => groups.push(vec![t]),
        }
    }

    for mut group in groups {
        // Ingress protocol first (passthrough), then the rest in stable order.
        group.sort_by_key(|t| t.protocol != req.ingress);
        for t in group {
            let reason = if !t.trust.allowed_for_inference() {
                Some("transport not allowed for inference")
            } else if stateful
                && (t.protocol != Protocol::Responses || req.ingress != Protocol::Responses)
            {
                Some("stateful request needs a native Responses target")
            } else if stateful && req.pinned_target.is_some_and(|p| p != &*t.id) {
                Some("stateful request is pinned to another target")
            } else if t
                .context_window
                .is_some_and(|cw| req.hints.estimated_input_tokens > cw)
            {
                Some("input exceeds context window")
            } else if !t.health.is_available(req.now_ms) {
                Some("unavailable (circuit breaker)")
            } else {
                None
            };
            match reason {
                Some(r) => out.skipped.push((t.id.clone(), r)),
                None => out.attempts.push((*t).clone()),
            }
        }
    }

    if resolution.kind == ResolutionKind::Exact
        && !req.allow_account_failover
        && let Some(named) = resolution.candidates.first()
    {
        out.attempts.retain(|t| t.account_id == named.account_id);
    }
    if stateful {
        // State lives on one upstream; never fail over (ADR-006 §3).
        out.attempts.truncate(1);
    } else {
        out.attempts.truncate(req.max_attempts.max(1));
    }
    out
}

#[cfg(test)]
mod tests {
    use loadout_protocol::hints::RouteHints;
    use loadout_provider_sdk::{
        Capabilities, Endpoint, FailureKind, ProviderConfig, ProviderDescriptor, ProviderModule,
        TrustLevel,
    };
    use serde_json::Value;

    use super::*;
    use crate::{HealthRegistry, Prices, RouteTable};

    struct Dummy;
    static DUMMY: ProviderDescriptor = ProviderDescriptor {
        kind: "dummy",
        display_name: "Dummy",
        fixed_slug: None,
        default_base_url: None,
        auth: loadout_provider_sdk::AuthMethod::ApiKey,
        transports: &[],
        drivers: loadout_provider_sdk::Drivers {
            discovery: false,
            auth: false,
            usage: false,
            catalog: false,
            transport: true,
            health: false,
        },
        capabilities: Capabilities::NONE,
        default_model_protocols: &[],
    };
    impl ProviderModule for Dummy {
        fn descriptor(&self) -> &'static ProviderDescriptor {
            &DUMMY
        }
    }

    fn target(
        reg: &HealthRegistry,
        provider: &str,
        alias: &str,
        pos: i64,
        model: &str,
        protocol: Protocol,
    ) -> Arc<RouteTarget> {
        let id: TargetId = Arc::from(format!("{provider}-{alias}-{model}:{}", protocol.as_str()));
        Arc::new(RouteTarget {
            health: reg.get_or_create(&id),
            id,
            provider_slug: Arc::from(provider),
            account_id: Arc::from(alias),
            account_alias: Arc::from(alias),
            account_position: pos,
            upstream_model: Arc::from(model),
            protocol,
            trust: TrustLevel::OfficialApi,
            context_window: Some(1000),
            max_output_tokens: None,
            capabilities: Capabilities::DEFAULT_MODEL,
            prices: Prices::default(),
            endpoint: Endpoint::new("https://x.test".into()),
            module: Arc::new(Dummy),
            config: Arc::new(ProviderConfig {
                slug: provider.into(),
                kind: "dummy".into(),
                base_url: None,
                settings: Value::Null,
            }),
        })
    }

    fn hints(model: &str) -> RouteHints {
        RouteHints {
            model: model.into(),
            stream: true,
            has_tools: false,
            has_images: false,
            wants_reasoning: false,
            previous_response_id: None,
            has_conversation: false,
            estimated_input_tokens: 10,
            client_wants_usage: false,
        }
    }

    fn request<'a>(ingress: Protocol, hints: &'a RouteHints) -> SelectRequest<'a> {
        SelectRequest {
            ingress,
            hints,
            now_ms: 0,
            max_attempts: 3,
            pinned_target: None,
            allow_account_failover: true,
        }
    }

    fn table(reg: &HealthRegistry) -> RouteTable {
        RouteTable::new(
            1,
            vec![
                target(reg, "cc", "backup", 1, "kimi", Protocol::Chat),
                target(reg, "cc", "main", 0, "kimi", Protocol::Messages),
                target(reg, "cc", "main", 0, "kimi", Protocol::Chat),
                target(reg, "cc", "main", 0, "vendor/deep", Protocol::Chat),
                target(reg, "ocg", "main", 0, "kimi", Protocol::Chat),
            ],
        )
    }

    fn ids(c: &Candidates) -> Vec<&str> {
        c.attempts.iter().map(|t| &*t.id).collect()
    }

    #[test]
    fn resolves_exact_provider_and_unknown_names() {
        let reg = HealthRegistry::default();
        let t = table(&reg);
        let exact = t.resolve("cc/main/kimi").expect("exact");
        assert_eq!(exact.kind, ResolutionKind::Exact);
        assert_eq!(
            exact.candidates.len(),
            3,
            "named account first, then other accounts"
        );
        assert_eq!(&*exact.candidates[2].account_alias, "backup");
        let backup_first = t.resolve("cc/backup/kimi").expect("exact");
        assert_eq!(&*backup_first.candidates[0].account_alias, "backup");

        let h = hints("cc/main/kimi");
        let mut no_failover = request(Protocol::Chat, &h);
        no_failover.allow_account_failover = false;
        assert_eq!(
            ids(&select(&exact, &no_failover)),
            vec!["cc-main-kimi:chat", "cc-main-kimi:messages"]
        );
        assert_eq!(
            select(&exact, &request(Protocol::Chat, &h)).attempts.len(),
            3
        );

        let any = t.resolve("cc/kimi").expect("provider/model");
        assert_eq!(any.kind, ResolutionKind::ProviderModel);
        assert_eq!(any.candidates.len(), 3, "main (2 protocols) then backup");
        assert_eq!(&*any.candidates[2].account_alias, "backup");

        assert!(
            t.resolve("cc/main/vendor/deep").is_ok(),
            "model ids may contain /"
        );
        assert!(t.resolve("cc/vendor/deep").is_ok());
        assert!(matches!(
            t.resolve("gpt-4o"),
            Err(crate::ResolveError::NotAddressed(_))
        ));
        assert!(matches!(
            t.resolve("cc/nope"),
            Err(crate::ResolveError::UnknownModel(_))
        ));
        assert!(matches!(
            t.resolve("loadout/realtime"),
            Err(crate::ResolveError::UnknownModel(_))
        ));
        assert!(t.model_names().contains(&"cc/main/kimi".to_owned()));
    }

    #[test]
    fn prefers_ingress_protocol_and_keeps_account_order() {
        let reg = HealthRegistry::default();
        let t = table(&reg);
        let h = hints("cc/kimi");
        let res = t.resolve("cc/kimi").expect("resolve");
        let c = select(&res, &request(Protocol::Messages, &h));
        assert_eq!(
            ids(&c),
            vec![
                "cc-main-kimi:messages",
                "cc-main-kimi:chat",
                "cc-backup-kimi:chat"
            ]
        );
        let c = select(&res, &request(Protocol::Chat, &h));
        assert_eq!(ids(&c)[0], "cc-main-kimi:chat");
    }

    #[test]
    fn skips_unhealthy_and_oversized_and_limits_attempts() {
        let reg = HealthRegistry::default();
        let t = table(&reg);
        reg.get("cc-main-kimi:chat").expect("h").record_failure(
            FailureKind::RateLimited { retry_after: None },
            "429",
            0,
        );
        let h = hints("cc/kimi");
        let res = t.resolve("cc/kimi").expect("resolve");
        let c = select(&res, &request(Protocol::Chat, &h));
        assert_eq!(
            ids(&c),
            vec!["cc-main-kimi:messages", "cc-backup-kimi:chat"]
        );
        assert_eq!(c.skipped.len(), 1);

        let mut big = hints("cc/kimi");
        big.estimated_input_tokens = 5000;
        let c = select(&res, &request(Protocol::Chat, &big));
        assert!(c.attempts.is_empty());
        assert!(
            c.skipped
                .iter()
                .all(|(_, r)| r.contains("context") || r.contains("breaker"))
        );

        let mut one = request(Protocol::Messages, &h);
        one.max_attempts = 1;
        assert_eq!(select(&res, &one).attempts.len(), 1);
    }

    #[test]
    fn stateful_requests_need_native_responses_and_never_fail_over() {
        let reg = HealthRegistry::default();
        let t = RouteTable::new(
            1,
            vec![
                target(&reg, "oa", "a", 0, "gpt", Protocol::Responses),
                target(&reg, "oa", "b", 1, "gpt", Protocol::Responses),
                target(&reg, "oa", "c", 2, "gpt", Protocol::Chat),
            ],
        );
        let mut h = hints("oa/gpt");
        h.previous_response_id = Some("resp_1".into());
        let res = t.resolve("oa/gpt").expect("resolve");

        let c = select(&res, &request(Protocol::Responses, &h));
        assert_eq!(ids(&c), vec!["oa-a-gpt:responses"]);

        let mut pinned = request(Protocol::Responses, &h);
        pinned.pinned_target = Some("oa-b-gpt:responses");
        assert_eq!(ids(&select(&res, &pinned)), vec!["oa-b-gpt:responses"]);

        assert!(
            select(&res, &request(Protocol::Chat, &h))
                .attempts
                .is_empty()
        );
    }

    #[test]
    fn virtual_models_follow_given_order() {
        let reg = HealthRegistry::default();
        let t = table(&reg).with_virtual(
            "fast",
            &[
                Arc::from("ocg-main-kimi:chat"),
                Arc::from("cc-backup-kimi:chat"),
                Arc::from("missing"),
            ],
        );
        let res = t.resolve("loadout/fast").expect("virtual");
        assert_eq!(res.kind, ResolutionKind::Virtual);
        let h = hints("loadout/fast");
        assert_eq!(
            ids(&select(&res, &request(Protocol::Chat, &h))),
            vec!["ocg-main-kimi:chat", "cc-backup-kimi:chat"]
        );
        assert_eq!(t.model_names()[0], "loadout/fast");
    }
}
