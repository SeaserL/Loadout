// SPDX-License-Identifier: Apache-2.0

//! Builds the route table from the database, the secret store and the provider registry
//! (ExecPlan 002 §4: credentials are read here, never on the request path).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use loadout_protocol::Protocol;
use loadout_provider_sdk::{AuthMethod, Capabilities, ProviderConfig};
use loadout_providers::Registry;
use loadout_router::{
    HealthRegistry, LatencyBook, Prices, RankedItem, RouteTable, RouteTarget, TargetId, Tier,
    account_model_key, order_by_latency,
};
use loadout_secrets::{Secret, SecretRef, SecretStore};
use loadout_storage::{Database, RoutableModel, StorageError};
use serde_json::Value;

/// Parses a `protocols_json` array; unknown names are ignored.
pub(crate) fn parse_protocols(json: &str) -> Vec<Protocol> {
    serde_json::from_str::<Vec<String>>(json)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| Protocol::parse(p))
        .collect()
}

pub(crate) fn protocols_json(protocols: &[Protocol]) -> String {
    serde_json::to_string(&protocols.iter().map(|p| p.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".into())
}

pub(crate) fn provider_config(p: &loadout_storage::Provider) -> ProviderConfig {
    ProviderConfig {
        slug: p.slug.clone(),
        kind: p.kind.clone(),
        base_url: p.base_url.clone(),
        settings: serde_json::from_str(&p.config_json).unwrap_or(Value::Object(Default::default())),
    }
}

/// Reads a secret by its stored reference; `None` (with a warning) when unreadable.
pub(crate) fn read_secret(secrets: &dyn SecretStore, secret_ref: &str) -> Option<Secret> {
    let parsed: SecretRef = secret_ref.parse().ok()?;
    match secrets.get(&parsed) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(secret_ref, error = %e, "cannot read credential");
            None
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct BuildReport {
    pub targets: usize,
    pub warnings: Vec<String>,
}

/// Built-in virtual models (ADR-003 §4–6, ExecPlan 004 §3.4).
pub(crate) const VIRTUAL_REALTIME: &str = "realtime";
pub(crate) const VIRTUAL_FAST: &str = "fast";
pub(crate) const VIRTUAL_DEFAULT: &str = "default";
/// Names reserved for built-in virtual models, now or later (ADR-003 Consequences).
pub(crate) const RESERVED_VIRTUAL_NAMES: &[&str] = &[
    VIRTUAL_REALTIME,
    VIRTUAL_FAST,
    VIRTUAL_DEFAULT,
    "balanced",
    "quality",
    "economy",
    "coding",
    "batch",
    "auto",
    "manual",
    "throughput",
];

/// One account model of a table: key, target ids, degraded, opted-in (lower trust).
type AccountModelEntry = (Arc<str>, Vec<TargetId>, bool, bool);

/// Account models of a table in table order, each with its target ids.
fn account_models(table: &RouteTable) -> Vec<AccountModelEntry> {
    let mut out: Vec<AccountModelEntry> = Vec::new();
    for t in table.targets() {
        let key = account_model_key(&t.id);
        let degraded = t.health.is_degraded();
        match out.iter_mut().find(|(k, ..)| &**k == key) {
            Some((_, ids, d, o)) => {
                ids.push(t.id.clone());
                *d |= degraded;
                *o |= t.opted_in;
            }
            None => out.push((Arc::from(key), vec![t.id.clone()], degraded, t.opted_in)),
        }
    }
    out
}

fn expand(items: &[RankedItem<usize>], models: &[AccountModelEntry]) -> Vec<TargetId> {
    items
        .iter()
        .flat_map(|i| models[i.key].1.iter().cloned())
        .collect()
}

/// Adds the built-in virtual models and the user's pools.
fn add_virtuals(
    mut table: RouteTable,
    latency: &LatencyBook,
    pools: &[loadout_storage::Pool],
) -> RouteTable {
    let models = account_models(&table);
    let ranked: Vec<RankedItem<usize>> = models
        .iter()
        .enumerate()
        .map(|(i, (key, _, degraded, _))| RankedItem {
            key: i,
            rank: latency.rank(key),
            degraded: *degraded,
            base_order: i,
        })
        .collect();
    // Opted-in lower-trust targets are only reachable by exact name or a user pool
    // (ADR-012 §1).
    let mut all: Vec<_> = ranked
        .iter()
        .filter(|i| !models[i.key].3)
        .cloned()
        .collect();
    order_by_latency(&mut all);
    let realtime: Vec<_> = all
        .iter()
        .filter(|i| i.rank.tier == Tier::UltraFast)
        .cloned()
        .collect();
    let fast: Vec<_> = all
        .iter()
        .filter(|i| i.rank.tier <= Tier::Fast)
        .cloned()
        .collect();
    table = table
        .with_virtual(VIRTUAL_REALTIME, &expand(&realtime, &models))
        .with_virtual(VIRTUAL_FAST, &expand(&fast, &models))
        .with_virtual(VIRTUAL_DEFAULT, &expand(&all, &models));

    for pool in pools {
        let mut members: Vec<RankedItem<usize>> = pool
            .members
            .iter()
            .enumerate()
            .filter_map(|(pos, id)| {
                let i = models.iter().position(|(k, ..)| &**k == id.as_str())?;
                Some(RankedItem {
                    base_order: pos,
                    ..ranked[i].clone()
                })
            })
            .collect();
        if pool.ordering == "latency" {
            order_by_latency(&mut members);
        }
        table = table.with_pool(&pool.name, &expand(&members, &models));
    }
    let tiers = models
        .iter()
        .zip(&ranked)
        .map(|((key, ..), item)| (key.clone(), item.rank.tier))
        .collect();
    table.with_tiers(tiers)
}

pub(crate) fn build(
    db: &Database,
    secrets: &dyn SecretStore,
    registry: &Registry,
    health: &HealthRegistry,
    latency: &LatencyBook,
    version: u64,
) -> Result<(RouteTable, BuildReport), StorageError> {
    let rows: Vec<RoutableModel> = db.routable_models()?;
    let mut report = BuildReport::default();
    let mut configs: HashMap<String, Arc<ProviderConfig>> = HashMap::new();
    let mut keys: HashMap<String, Option<Secret>> = HashMap::new();
    let mut targets = Vec::new();
    let mut ids: HashSet<TargetId> = HashSet::new();

    for row in rows {
        let Some(module) = registry.get(&row.provider.kind) else {
            report.warnings.push(format!(
                "provider {} uses unknown kind {}",
                row.provider.slug, row.provider.kind
            ));
            continue;
        };
        let descriptor = module.descriptor();
        let config = configs
            .entry(row.provider.id.clone())
            .or_insert_with(|| Arc::new(provider_config(&row.provider)))
            .clone();
        let key = match &row.secret_ref {
            Some(r) => keys
                .entry(r.clone())
                .or_insert_with(|| read_secret(secrets, r))
                .as_ref(),
            None => None,
        };
        if descriptor.auth == AuthMethod::ApiKey && key.is_none() {
            report.warnings.push(format!(
                "{}/{}: no usable API key; its models are not routable",
                row.provider.slug, row.account_alias
            ));
            continue;
        }
        if descriptor.auth == AuthMethod::OAuth && key.is_none() {
            report.warnings.push(format!(
                "{}/{}: not signed in; its models are not routable",
                row.provider.slug, row.account_alias
            ));
            continue;
        }

        let mut protocols = parse_protocols(&row.model.protocols_json);
        if protocols.is_empty() {
            protocols = descriptor.default_model_protocols.to_vec();
        }
        let capabilities =
            Capabilities::from_json(&row.model.capabilities_json).union(Capabilities::STREAMING);
        for protocol in protocols {
            if descriptor.transport(protocol).is_none() {
                continue;
            }
            let Some(trust) = module.trust(&config, protocol) else {
                continue;
            };
            // Lower-trust transports need the account owner's confirmation (ADR-012).
            let opted_in = !trust.allowed_for_inference();
            if opted_in && !row.transport_opt_in {
                continue;
            }
            let endpoint = match module.endpoint(&config, protocol, key) {
                Ok(e) => e,
                Err(e) => {
                    report.warnings.push(format!(
                        "{}/{}/{} ({protocol}): {e}",
                        row.provider.slug, row.account_alias, row.model.upstream_model_id
                    ));
                    continue;
                }
            };
            let id: TargetId = Arc::from(format!("{}:{}", row.model.id, protocol.as_str()));
            ids.insert(id.clone());
            targets.push(Arc::new(RouteTarget {
                health: health.get_or_create(&id),
                id,
                provider_slug: Arc::from(row.provider.slug.as_str()),
                account_id: Arc::from(row.account_id.as_str()),
                account_alias: Arc::from(row.account_alias.as_str()),
                account_position: row.account_position,
                upstream_model: Arc::from(row.model.upstream_model_id.as_str()),
                protocol,
                trust,
                opted_in,
                context_window: row.model.context_window.and_then(|c| u32::try_from(c).ok()),
                max_output_tokens: row
                    .model
                    .max_output_tokens
                    .and_then(|c| u32::try_from(c).ok()),
                capabilities,
                prices: Prices {
                    input: row.model.input_price,
                    output: row.model.output_price,
                    cache_read: row.model.cache_read_price,
                    cache_write: row.model.cache_write_price,
                },
                endpoint,
                module: module.clone(),
                config: config.clone(),
            }));
        }
    }
    health.retain(|id| ids.contains(id));
    let models: HashSet<&str> = ids.iter().map(|id| account_model_key(id)).collect();
    latency.retain(|key| models.contains(key));
    report.targets = targets.len();
    let pools = db.pools()?;
    Ok((
        add_virtuals(RouteTable::new(version, targets), latency, &pools),
        report,
    ))
}
