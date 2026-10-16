// SPDX-License-Identifier: Apache-2.0

//! `account.check` (ExecPlan 014): tests an account's credentials with its catalog call and,
//! on request, every enabled model with a 1-token probe. Runs in the control plane only;
//! the request hot path never waits for it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{AccountCheckResult, ModelCheck};
use loadout_provider_sdk::{DriverContext, FailureKind, ProviderError};
use loadout_router::{HealthState, RouteTarget, now_ms};

use crate::bench::StreamHealth;
use crate::control::{ControlPlane, blocking, key_ref, storage};
use crate::route_build::{provider_config, read_secret};

/// The catalog call is a single small request; anything slower counts as unreachable.
const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);

/// Maps a catalog failure to `(status, http status, message)`.
fn catalog_status(e: &ProviderError) -> (&'static str, Option<u16>, String) {
    match e {
        ProviderError::Upstream(f) => {
            let status = match f.kind {
                FailureKind::Auth => "invalid_key",
                FailureKind::AccountDisabled => "account_disabled",
                FailureKind::RateLimited { .. } => "rate_limited",
                FailureKind::QuotaExhausted { .. } => "no_credit",
                FailureKind::Timeout | FailureKind::Connect => "unreachable",
                FailureKind::Server
                | FailureKind::ModelNotFound
                | FailureKind::ModelUnavailable
                | FailureKind::InvalidRequest
                | FailureKind::Other => "upstream_error",
            };
            (status, f.status, f.message.clone())
        }
        ProviderError::NotSupported(_) => (
            "unsupported",
            None,
            "this provider has no model list call; check the models instead".to_owned(),
        ),
        ProviderError::MissingKey => ("missing_key", None, e.to_string()),
        ProviderError::Config(_) | ProviderError::InvalidResponse(_) => {
            ("upstream_error", None, e.to_string())
        }
    }
}

impl ControlPlane {
    pub async fn check_account(
        &self,
        provider: String,
        alias: String,
        models: bool,
    ) -> Result<AccountCheckResult, ErrorBody> {
        let (db, secrets) = (self.db.clone(), self.secrets.clone());
        let (p_slug, a_alias) = (provider.clone(), alias.clone());
        let (provider_row, account, key) = blocking(move || {
            let p = Self::load_provider(&db, &p_slug)?;
            let a = Self::load_account(&db, &p, &a_alias)?;
            let key = key_ref(db.accounts().credentials(&a.id).map_err(storage)?)
                .and_then(|r| read_secret(secrets.as_ref(), &r));
            Ok((p, a, key))
        })
        .await?;
        let module = self
            .registry
            .get(&provider_row.kind)
            .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?
            .clone();
        let config = provider_config(&provider_row);

        let started = Instant::now();
        let listed = tokio::time::timeout(
            CATALOG_TIMEOUT,
            module.list_models(DriverContext {
                http: &self.http,
                config: &config,
                key: key.as_ref(),
                home: Some(&self.home),
            }),
        )
        .await;
        let duration_ms = started.elapsed().as_millis() as u64;
        let (status, http_status, message, models_listed) = match listed {
            Ok(Ok(list)) => ("ok", None, None, Some(list.len())),
            Ok(Err(e)) => {
                let (s, code, m) = catalog_status(&e);
                (s, code, Some(m), None)
            }
            Err(_) => (
                "unreachable",
                None,
                Some("the model list request timed out".to_owned()),
                None,
            ),
        };

        let account_targets: Vec<Arc<RouteTarget>> = self
            .routes
            .load()
            .targets()
            .iter()
            .filter(|t| *t.account_id == *account.id)
            .cloned()
            .collect();
        let had_auth_failure = account_targets
            .iter()
            .any(|t| t.health.state() == HealthState::AuthFailed);
        match (status, message.as_deref()) {
            // The key works: lift auth failures, including terminal ones.
            ("ok", _) => self.reset_account_health(&account.id, true),
            // The verdict also feeds the breaker, like a failed request would.
            ("invalid_key", Some(m)) => account_targets
                .iter()
                .for_each(|t| t.health.record_failure(FailureKind::Auth, m, now_ms())),
            ("account_disabled", Some(m)) => account_targets
                .iter()
                .for_each(|t| t.health.mark_auth_terminal(m, now_ms())),
            _ => {}
        }

        let mut checks = Vec::new();
        if models {
            let picked = Self::unique_models(account_targets.iter());
            let results = self.probe_all(picked).await;
            if let Err(e) = self.evaluate_latency(false).await {
                tracing::warn!(error = %e.message, "latency evaluation after a check failed");
            }
            for (t, o) in results {
                let answered = o.ok || o.stream == Some(StreamHealth::Clean);
                checks.push(ModelCheck {
                    name: t.exact_name(),
                    protocol: t.protocol.as_str().to_owned(),
                    ok: answered,
                    ttft_ms: o.ttft_ms,
                    duration_ms: o.duration_ms,
                    error: o.error,
                    stream: o.stream.map(|s| s.as_str().to_owned()),
                });
            }
            checks.sort_by(|a, b| a.name.cmp(&b.name));
            // Without a catalog call, a model that answers proves the credentials.
            if status == "unsupported" && checks.iter().any(|c| c.ok) {
                self.reset_account_health(&account.id, true);
            }
        }
        let cleared = had_auth_failure
            && !account_targets
                .iter()
                .any(|t| t.health.state() == HealthState::AuthFailed);

        tracing::info!(
            provider = %provider,
            alias = %alias,
            status,
            models = checks.len(),
            "account check"
        );
        Ok(AccountCheckResult {
            account: format!("{provider}/{alias}"),
            status: status.to_owned(),
            http_status,
            message,
            models_listed,
            duration_ms,
            cleared,
            models: checks,
        })
    }
}

#[cfg(test)]
mod tests {
    use loadout_provider_sdk::UpstreamFailure;

    use super::*;

    #[test]
    fn catalog_failures_map_to_check_statuses() {
        let up = |kind, status| {
            catalog_status(&ProviderError::Upstream(UpstreamFailure::new(
                kind,
                Some(status),
                "m",
            )))
            .0
        };
        assert_eq!(up(FailureKind::Auth, 401), "invalid_key");
        assert_eq!(up(FailureKind::AccountDisabled, 403), "account_disabled");
        assert_eq!(
            up(FailureKind::RateLimited { retry_after: None }, 429),
            "rate_limited"
        );
        assert_eq!(
            up(FailureKind::QuotaExhausted { reset_after: None }, 402),
            "no_credit"
        );
        assert_eq!(up(FailureKind::Connect, 0), "unreachable");
        assert_eq!(up(FailureKind::Server, 500), "upstream_error");
        assert_eq!(
            catalog_status(&ProviderError::NotSupported("catalog")).0,
            "unsupported"
        );
        assert_eq!(catalog_status(&ProviderError::MissingKey).0, "missing_key");
    }
}
