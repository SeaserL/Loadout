// SPDX-License-Identifier: Apache-2.0

//! Client policies (ExecPlan 006 §3.1): stored as JSON, validated here, enforced by the
//! gateway from an in-memory snapshot.

use loadout_gateway::{ClientId, ClientPolicies, ClientPolicy as GatewayPolicy};
use loadout_ipc::ErrorBody;
use loadout_ipc::api::ClientPolicy;
use loadout_storage::{Client, Database, StorageError};

/// A stored policy; malformed JSON (edited by hand) counts as no policy.
pub(crate) fn parse_policy(json: &str) -> ClientPolicy {
    serde_json::from_str(json).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "ignoring malformed client policy");
        ClientPolicy::default()
    })
}

pub(crate) fn policy_json(p: &ClientPolicy) -> String {
    serde_json::to_string(p).unwrap_or_else(|_| "{}".to_owned())
}

fn to_gateway(p: ClientPolicy) -> GatewayPolicy {
    GatewayPolicy {
        default_model: p.default_model,
        allowed_models: p.allowed_models,
        daily_token_limit: p.daily_token_limit,
        daily_cost_limit_usd: p.daily_cost_limit_usd,
        requests_per_minute: p.requests_per_minute,
    }
}

/// Normalizes and checks a policy from the user.
pub(crate) fn validate(mut p: ClientPolicy) -> Result<ClientPolicy, ErrorBody> {
    let bad = |m: &str| Err(ErrorBody::invalid_params(m.to_owned()));
    p.default_model = p
        .default_model
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    if let Some(m) = &p.default_model
        && loadout_router::parse_name(m).is_none()
    {
        return bad(
            "default model must be a Loadout name such as loadout/default or provider/account/model",
        );
    }
    p.allowed_models = p
        .allowed_models
        .iter()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .collect();
    p.allowed_models.dedup();
    if let Some(m) = &p.default_model
        && !to_gateway(p.clone()).allows(m)
    {
        return bad("the default model is not among the allowed models");
    }
    if p.daily_token_limit == Some(0) || p.requests_per_minute == Some(0) {
        return bad("limits must be greater than zero (leave them empty for no limit)");
    }
    if let Some(c) = p.daily_cost_limit_usd
        && (!c.is_finite() || c <= 0.0)
    {
        return bad("the daily cost limit must be a positive number");
    }
    Ok(p)
}

/// Publishes the policies of all enabled clients to the gateway.
pub(crate) fn publish(policies: &ClientPolicies, clients: &[Client]) {
    policies.replace(clients.iter().filter(|c| c.enabled).map(|c| {
        (
            ClientId::from(c.id.as_str()),
            to_gateway(parse_policy(&c.policy_json)),
        )
    }));
}

/// Restores today's (UTC) usage counters after a restart.
pub(crate) fn seed_usage(
    policies: &ClientPolicies,
    db: &Database,
    now_ms: i64,
) -> Result<usize, StorageError> {
    let day_start = now_ms.div_euclid(86_400_000) * 86_400_000;
    let rows = db.clients().usage_since(day_start)?;
    for (id, tokens, cost) in &rows {
        policies.record(id, *tokens, *cost, now_ms);
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_normalizes_and_rejects() {
        let ok = validate(ClientPolicy {
            default_model: Some(" loadout/fast ".into()),
            allowed_models: vec!["loadout/*".into(), " ".into(), "loadout/*".into()],
            ..ClientPolicy::default()
        })
        .expect("valid");
        assert_eq!(ok.default_model.as_deref(), Some("loadout/fast"));
        assert_eq!(ok.allowed_models, vec!["loadout/*"]);

        for bad in [
            ClientPolicy {
                default_model: Some("gpt-4o".into()),
                ..ClientPolicy::default()
            },
            ClientPolicy {
                default_model: Some("x/y/z".into()),
                allowed_models: vec!["loadout/*".into()],
                ..ClientPolicy::default()
            },
            ClientPolicy {
                requests_per_minute: Some(0),
                ..ClientPolicy::default()
            },
            ClientPolicy {
                daily_cost_limit_usd: Some(f64::NAN),
                ..ClientPolicy::default()
            },
        ] {
            assert!(validate(bad.clone()).is_err(), "{bad:?}");
        }
        assert_eq!(parse_policy("not json"), ClientPolicy::default());
        let json = policy_json(&ok);
        assert_eq!(parse_policy(&json), ok);
    }
}
