// SPDX-License-Identifier: Apache-2.0

//! Dedicated endpoints (ADR-015, ExecPlan 021): management, and the recovery watcher that
//! probes rate-limited or cooling member targets when their wait ends.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use loadout_ipc::ErrorBody;
use loadout_ipc::api::EndpointInfo;
use loadout_router::{
    HealthState, RouteTable, RouteTarget, account_model_key, now_ms, validate_slug,
};
use loadout_storage::{Database, EndpointRow};
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, storage};

const DEFAULT_WAIT_SECS: u32 = 30;
const MAX_WAIT_SECS: u32 = 300;
/// How often the recovery watcher looks for targets whose wait has ended.
const WATCH_EVERY: Duration = Duration::from_secs(5);
/// Failures older than this are not chased by the watcher (no recent traffic).
const WATCH_RECENT_MS: i64 = 6 * 3_600_000;

fn info(
    row: &EndpointRow,
    db: &Database,
    table: &RouteTable,
    port: u16,
) -> Result<EndpointInfo, ErrorBody> {
    let provider = ControlPlane::load_provider(db, &row.provider_slug)?;
    let accounts = db.accounts().list(Some(&provider.id)).map_err(storage)?;
    let alias_of: HashMap<&str, &str> = accounts
        .iter()
        .map(|a| (a.id.as_str(), a.alias.as_str()))
        .collect();
    let members = row.members.as_ref().map(|ids| {
        ids.iter()
            .filter_map(|id| alias_of.get(id.as_str()).map(|a| (*a).to_owned()))
            .collect()
    });
    let mut active: Vec<String> = Vec::new();
    if row.enabled {
        for t in table.targets() {
            let member = row
                .members
                .as_ref()
                .is_none_or(|m| m.iter().any(|id| id.as_str() == &*t.account_id));
            if *t.provider_slug == *row.provider_slug
                && member
                && !active.iter().any(|a| a.as_str() == &*t.account_alias)
            {
                active.push(t.account_alias.to_string());
            }
        }
    }
    let base = format!("http://127.0.0.1:{port}/e/{}", row.name);
    Ok(EndpointInfo {
        name: row.name.clone(),
        provider: row.provider_slug.clone(),
        enabled: row.enabled,
        members,
        default_model: row.default_model.clone(),
        wait_secs: row.wait_secs,
        active_accounts: active,
        models: table
            .endpoint_models(&row.name)
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        openai_base_url: format!("{base}/v1"),
        anthropic_base_url: base,
    })
}

impl ControlPlane {
    pub async fn list_endpoints(&self, port: u16) -> Result<Vec<EndpointInfo>, ErrorBody> {
        let (db, routes) = (self.db.clone(), self.routes.clone());
        blocking(move || {
            let table = routes.load();
            db.endpoints()
                .list()
                .map_err(storage)?
                .iter()
                .map(|r| info(r, &db, &table, port))
                .collect()
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_endpoint(
        &self,
        port: u16,
        name: Option<String>,
        provider: String,
        enabled: bool,
        members: Option<Vec<String>>,
        default_model: Option<String>,
        wait_secs: Option<u32>,
    ) -> Result<EndpointInfo, ErrorBody> {
        let name = name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| provider.clone());
        validate_slug(&name).map_err(|e| {
            ErrorBody::invalid_params(e.to_string().replace("provider slug", "endpoint name"))
        })?;
        let wait_secs = wait_secs.unwrap_or(DEFAULT_WAIT_SECS);
        if wait_secs > MAX_WAIT_SECS {
            return Err(ErrorBody::invalid_params(format!(
                "wait_secs must be 0-{MAX_WAIT_SECS}"
            )));
        }
        let db = self.db.clone();
        let n = name.clone();
        blocking(move || {
            let p = Self::load_provider(&db, &provider)?;
            let members = match members {
                None => None,
                Some(aliases) => {
                    let mut ids = Vec::new();
                    for alias in aliases {
                        ids.push(Self::load_account(&db, &p, alias.trim())?.id);
                    }
                    Some(ids)
                }
            };
            db.endpoints()
                .save(&EndpointRow {
                    name: n,
                    provider_slug: p.slug,
                    enabled,
                    members,
                    default_model: default_model
                        .map(|m| m.trim().to_owned())
                        .filter(|m| !m.is_empty()),
                    wait_secs,
                })
                .map_err(storage)
        })
        .await?;
        tracing::info!(endpoint = %name, enabled, "dedicated endpoint saved");
        self.rebuild_routes().await?;
        self.list_endpoints(port)
            .await?
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| ErrorBody::internal("endpoint vanished"))
    }

    pub async fn remove_endpoint(&self, name: String) -> Result<bool, ErrorBody> {
        let db = self.db.clone();
        let removed = blocking(move || db.endpoints().delete(&name).map_err(storage)).await?;
        if removed {
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    /// Member targets of enabled endpoints whose blocking state has ended and that have not
    /// been confirmed since: one per account model, the chat protocol first.
    fn recovery_candidates(&self) -> Vec<Arc<RouteTarget>> {
        let table = self.routes.load();
        let now = now_ms();
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        let mut members = table.endpoint_targets();
        members.sort_by_key(|t| t.protocol != loadout_protocol::Protocol::Chat);
        for t in members {
            let snap = t.health.snapshot();
            let blocked = matches!(
                snap.state,
                HealthState::RateLimited | HealthState::Cooldown | HealthState::QuotaExhausted
            ) || (snap.state == HealthState::AuthFailed && !snap.terminal);
            let due = snap.until_ms.is_some_and(|u| u <= now);
            let recent = snap
                .last_failure_ms
                .is_some_and(|f| now - f <= WATCH_RECENT_MS);
            if blocked && due && recent && seen.insert(account_model_key(&t.id).to_owned()) {
                out.push(t.clone());
            }
        }
        out
    }

    /// Probes recovering endpoint members until shutdown (ADR-015 §4).
    pub(crate) async fn watch_recovery(
        self: Arc<Self>,
        shutdown: CancellationToken,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(WATCH_EVERY) => {}
            }
            for t in self.recovery_candidates() {
                let outcome = crate::bench::probe(&self.http, &t).await;
                tracing::info!(
                    target = %t.exact_name(),
                    ok = outcome.ok,
                    "recovery probe"
                );
                if outcome.failure == Some(loadout_provider_sdk::FailureKind::ModelUnavailable) {
                    self.mark_model_unavailable(&t.id).await;
                }
                if outcome.ok {
                    // Same key and model over another protocol: recovered too.
                    let key = account_model_key(&t.id);
                    for sibling in self.routes.load().targets() {
                        if sibling.id != t.id
                            && account_model_key(&sibling.id) == key
                            && !sibling.health.is_terminal()
                            && sibling.health.state() != HealthState::Healthy
                        {
                            sibling.health.record_success(now_ms());
                        }
                    }
                }
            }
        }
    }
}
