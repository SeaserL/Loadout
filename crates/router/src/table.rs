// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::{ParsedName, RouteTarget, TargetId, Tier, account_model_key, parse_name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionKind {
    /// `provider/account/model`
    Exact,
    /// `provider/model`: that model on any of the provider's accounts, in account order.
    ProviderModel,
    /// `loadout/<name>`
    Virtual,
    /// A model inside a dedicated endpoint (`/e/<name>/v1`, ADR-015): that model on the
    /// endpoint's member accounts, one attempt per account.
    Endpoint,
}

#[derive(Debug)]
pub struct Resolution<'a> {
    pub kind: ResolutionKind,
    /// Ordered candidates (all protocols of each account model).
    pub candidates: Vec<&'a Arc<RouteTarget>>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error(
        "model \"{0}\" is not a Loadout model name; use provider/account/model, provider/model or loadout/<name> (see GET /v1/models)"
    )]
    NotAddressed(String),
    #[error("model \"{0}\" not found (see GET /v1/models)")]
    UnknownModel(String),
    #[error("model \"{model}\" is not served by endpoint {endpoint}; available: {available}")]
    NotInEndpoint {
        endpoint: String,
        model: String,
        available: String,
    },
    #[error("endpoint \"{0}\" does not exist or is turned off")]
    UnknownEndpoint(String),
}

/// A dedicated endpoint's index (ADR-015): upstream model -> member targets in table order.
#[derive(Debug, Default)]
struct EndpointIndex {
    provider: String,
    models: HashMap<String, Vec<usize>>,
    default_model: Option<String>,
    accounts: usize,
    /// How long a request may wait for a cooling member to recover.
    wait_secs: u32,
}

/// Immutable routing table built by the control plane.
#[derive(Debug, Default)]
pub struct RouteTable {
    version: u64,
    targets: Vec<Arc<RouteTarget>>,
    by_id: HashMap<TargetId, usize>,
    exact: HashMap<String, Vec<usize>>,
    provider_model: HashMap<String, Vec<usize>>,
    virtuals: HashMap<String, Vec<usize>>,
    /// Which virtual names are user pools (the rest are built-in virtual models).
    pools: HashSet<String>,
    /// Latency tier per account model at build time (display only).
    tiers: HashMap<Arc<str>, Tier>,
    /// Dedicated endpoints by name (ADR-015).
    endpoints: HashMap<String, EndpointIndex>,
}

/// What a listed model name addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ListingKind {
    /// Built-in `loadout/<name>`.
    Virtual,
    /// User pool `loadout/<name>`.
    Pool,
    /// `provider/account/model`.
    Exact,
}

impl ListingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Virtual => "virtual",
            Self::Pool => "pool",
            Self::Exact => "exact",
        }
    }
}

/// One entry of the model listing (`GET /v1/models`).
#[derive(Debug)]
pub struct ModelListing<'a> {
    pub name: String,
    pub kind: ListingKind,
    /// Virtual / pool: the ordered chain. Exact: the account model's protocol variants.
    pub targets: Vec<&'a Arc<RouteTarget>>,
}

impl RouteTable {
    /// Builds indexes. Targets are ordered by provider, account position, alias, model and
    /// protocol so resolution order is deterministic and follows the user's account order.
    pub fn new(version: u64, mut targets: Vec<Arc<RouteTarget>>) -> Self {
        targets.sort_by(|a, b| {
            (
                &a.provider_slug,
                a.account_position,
                &a.account_alias,
                &a.upstream_model,
                a.protocol,
            )
                .cmp(&(
                    &b.provider_slug,
                    b.account_position,
                    &b.account_alias,
                    &b.upstream_model,
                    b.protocol,
                ))
        });
        let mut table = Self {
            version,
            ..Self::default()
        };
        for (i, t) in targets.iter().enumerate() {
            table.by_id.insert(t.id.clone(), i);
            table.exact.entry(t.exact_name()).or_default().push(i);
            table
                .provider_model
                .entry(format!("{}/{}", t.provider_slug, t.upstream_model))
                .or_default()
                .push(i);
        }
        table.targets = targets;
        table
    }

    /// Adds a virtual model (`loadout/<name>`) with an ordered list of target ids. Unknown
    /// ids are ignored.
    pub fn with_virtual(mut self, name: &str, target_ids: &[TargetId]) -> Self {
        let indexes: Vec<usize> = target_ids
            .iter()
            .filter_map(|id| self.by_id.get(id).copied())
            .collect();
        self.virtuals.insert(name.to_owned(), indexes);
        self
    }

    /// Like [`Self::with_virtual`], marking the name as a user pool.
    pub fn with_pool(mut self, name: &str, target_ids: &[TargetId]) -> Self {
        self.pools.insert(name.to_owned());
        self.with_virtual(name, target_ids)
    }

    /// Adds a dedicated endpoint (ADR-015) over the provider's targets, limited to
    /// `members` (account ids) when given.
    pub fn with_endpoint(
        mut self,
        name: &str,
        provider: &str,
        members: Option<&HashSet<String>>,
        default_model: Option<&str>,
        wait_secs: u32,
    ) -> Self {
        let mut index = EndpointIndex {
            provider: provider.to_owned(),
            default_model: default_model.map(str::to_owned),
            wait_secs,
            ..EndpointIndex::default()
        };
        let mut accounts: HashSet<&str> = HashSet::new();
        for (i, t) in self.targets.iter().enumerate() {
            if &*t.provider_slug != provider || members.is_some_and(|m| !m.contains(&*t.account_id))
            {
                continue;
            }
            accounts.insert(&t.account_id);
            index
                .models
                .entry(t.upstream_model.to_string())
                .or_default()
                .push(i);
        }
        index.accounts = accounts.len();
        self.endpoints.insert(name.to_owned(), index);
        self
    }

    /// Resolves a model name inside a dedicated endpoint (ADR-015 §2): the bare upstream id,
    /// `provider/model`, or `provider/account/model` (that account first), else the
    /// endpoint's default model.
    pub fn resolve_endpoint(
        &self,
        endpoint: &str,
        model: &str,
    ) -> Result<Resolution<'_>, ResolveError> {
        let ep = self
            .endpoints
            .get(endpoint)
            .ok_or_else(|| ResolveError::UnknownEndpoint(endpoint.to_owned()))?;
        let pick = |idx: &[usize]| idx.iter().map(|&i| &self.targets[i]).collect::<Vec<_>>();
        let found = |m: &str| ep.models.get(m).filter(|v| !v.is_empty());
        let mut first_account: Option<&str> = None;
        let mut idx = found(model);
        if idx.is_none()
            && let Some(rest) = model
                .strip_prefix(ep.provider.as_str())
                .and_then(|r| r.strip_prefix('/'))
        {
            idx = found(rest);
            if idx.is_none()
                && let Some(&i) = self.exact.get(model).and_then(|v| v.first())
            {
                let t = &self.targets[i];
                idx = found(&t.upstream_model);
                first_account = Some(&t.account_id);
            }
        }
        if idx.is_none()
            && let Some(d) = &ep.default_model
        {
            idx = found(d);
        }
        let Some(idx) = idx else {
            return Err(ResolveError::NotInEndpoint {
                endpoint: endpoint.to_owned(),
                model: model.to_owned(),
                available: self
                    .endpoint_models(endpoint)
                    .unwrap_or_default()
                    .join(", "),
            });
        };
        let mut candidates = pick(idx);
        if let Some(a) = first_account {
            candidates.sort_by_key(|t| &*t.account_id != a);
        }
        Ok(Resolution {
            kind: ResolutionKind::Endpoint,
            candidates,
        })
    }

    /// Upstream model ids served by an endpoint, sorted; `None` when it does not exist.
    pub fn endpoint_models(&self, endpoint: &str) -> Option<Vec<&str>> {
        let ep = self.endpoints.get(endpoint)?;
        let mut names: Vec<&str> = ep.models.keys().map(String::as_str).collect();
        names.sort_unstable();
        Some(names)
    }

    /// How long a request to the endpoint may wait for a member to recover (ADR-015 §3).
    pub fn endpoint_wait_secs(&self, endpoint: &str) -> u32 {
        self.endpoints.get(endpoint).map_or(0, |e| e.wait_secs)
    }

    /// Every target that belongs to at least one endpoint, in table order.
    pub fn endpoint_targets(&self) -> Vec<&Arc<RouteTarget>> {
        let mut idx: Vec<usize> = self
            .endpoints
            .values()
            .flat_map(|e| e.models.values().flatten().copied())
            .collect();
        idx.sort_unstable();
        idx.dedup();
        idx.into_iter().map(|i| &self.targets[i]).collect()
    }

    /// The provider an endpoint serves.
    pub fn endpoint_provider(&self, endpoint: &str) -> Option<&str> {
        self.endpoints.get(endpoint).map(|e| e.provider.as_str())
    }

    /// How many member accounts an endpoint has in this table.
    pub fn endpoint_accounts(&self, endpoint: &str) -> usize {
        self.endpoints.get(endpoint).map_or(0, |e| e.accounts)
    }

    /// Records the latency tier of each account model (display only; the routing order
    /// is already part of the virtual chains).
    pub fn with_tiers(mut self, tiers: HashMap<Arc<str>, Tier>) -> Self {
        self.tiers = tiers;
        self
    }

    /// Tier of a target's account model when the table was built.
    pub fn tier(&self, target: &RouteTarget) -> Tier {
        self.tiers
            .get(account_model_key(&target.id))
            .copied()
            .unwrap_or(Tier::Unmeasured)
    }

    /// Everything `GET /v1/models` lists: built-in virtual models, then pools, then exact
    /// names, each group sorted by name. Empty virtual models are left out.
    pub fn listing(&self) -> Vec<ModelListing<'_>> {
        let pick = |idx: &[usize]| idx.iter().map(|&i| &self.targets[i]).collect::<Vec<_>>();
        let mut out: Vec<ModelListing<'_>> = self
            .virtuals
            .iter()
            .filter(|(_, idx)| !idx.is_empty())
            .map(|(k, idx)| ModelListing {
                name: format!("loadout/{k}"),
                kind: if self.pools.contains(k) {
                    ListingKind::Pool
                } else {
                    ListingKind::Virtual
                },
                targets: pick(idx),
            })
            .chain(self.exact.iter().map(|(k, idx)| ModelListing {
                name: k.clone(),
                kind: ListingKind::Exact,
                targets: pick(idx),
            }))
            .collect();
        out.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        out
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn targets(&self) -> &[Arc<RouteTarget>] {
        &self.targets
    }

    pub fn target(&self, id: &str) -> Option<&Arc<RouteTarget>> {
        self.by_id.get(id).map(|&i| &self.targets[i])
    }

    pub fn resolve(&self, name: &str) -> Result<Resolution<'_>, ResolveError> {
        let pick = |idx: &[usize]| idx.iter().map(|&i| &self.targets[i]).collect::<Vec<_>>();
        match parse_name(name) {
            None => Err(ResolveError::NotAddressed(name.to_owned())),
            Some(ParsedName::Virtual(v)) => self
                .virtuals
                .get(v)
                .map(|idx| Resolution {
                    kind: ResolutionKind::Virtual,
                    candidates: pick(idx),
                })
                .ok_or_else(|| ResolveError::UnknownModel(name.to_owned())),
            Some(ParsedName::Provider { .. }) => {
                // Alias validation (ADR-010 §3) guarantees an exact name never also parses
                // as provider/model with a different meaning, so exact wins.
                if let Some(idx) = self.exact.get(name) {
                    // The named account first, then the same model on the provider's other
                    // accounts (ADR-003 §8); `select` trims these when failover is off.
                    let mut candidates = pick(idx);
                    let first = candidates[0];
                    let same_model = format!("{}/{}", first.provider_slug, first.upstream_model);
                    if let Some(others) = self.provider_model.get(&same_model) {
                        candidates.extend(
                            others
                                .iter()
                                .map(|&i| &self.targets[i])
                                .filter(|t| t.account_id != first.account_id),
                        );
                    }
                    return Ok(Resolution {
                        kind: ResolutionKind::Exact,
                        candidates,
                    });
                }
                self.provider_model
                    .get(name)
                    .map(|idx| Resolution {
                        kind: ResolutionKind::ProviderModel,
                        candidates: pick(idx),
                    })
                    .ok_or_else(|| ResolveError::UnknownModel(name.to_owned()))
            }
        }
    }

    /// Virtual models and pools (without the `loadout/` prefix) with their ordered targets,
    /// sorted by name.
    pub fn virtuals(&self) -> Vec<(&str, Vec<&Arc<RouteTarget>>)> {
        let mut out: Vec<(&str, Vec<&Arc<RouteTarget>>)> = self
            .virtuals
            .iter()
            .map(|(k, idx)| (k.as_str(), idx.iter().map(|&i| &self.targets[i]).collect()))
            .collect();
        out.sort_by_key(|(k, _)| *k);
        out
    }

    /// Names for `GET /v1/models`: virtual models, then exact names.
    pub fn model_names(&self) -> Vec<String> {
        let mut virtuals: Vec<String> = self
            .virtuals
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, _)| format!("loadout/{k}"))
            .collect();
        virtuals.sort();
        let mut exact: Vec<String> = self.exact.keys().cloned().collect();
        exact.sort();
        virtuals.extend(exact);
        virtuals
    }
}

/// Lock-free publication of the current table (ADR-003 §2).
#[derive(Debug, Default)]
pub struct RouteTableHandle {
    current: ArcSwap<RouteTable>,
}

impl RouteTableHandle {
    pub fn new(table: RouteTable) -> Self {
        Self {
            current: ArcSwap::from_pointee(table),
        }
    }

    pub fn load(&self) -> Arc<RouteTable> {
        self.current.load_full()
    }

    pub fn store(&self, table: RouteTable) {
        self.current.store(Arc::new(table));
    }
}
