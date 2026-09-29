// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::{ParsedName, RouteTarget, TargetId, parse_name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionKind {
    /// `provider/account/model`
    Exact,
    /// `provider/model`: that model on any of the provider's accounts, in account order.
    ProviderModel,
    /// `loadout/<name>`
    Virtual,
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
