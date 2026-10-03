// SPDX-License-Identifier: Apache-2.0

//! Data-plane routing (ADR-003, ADR-010).
//!
//! The control plane builds an immutable [`RouteTable`] and publishes it through a
//! [`RouteTableHandle`] (ArcSwap). A request resolves its model name against the current
//! table and gets an ordered list of candidate [`RouteTarget`]s filtered by eligibility and
//! per-target health. Everything here is in-memory; nothing touches disk or network.

mod health;
mod latency;
mod name;
mod select;
mod session;
mod table;
mod target;

pub use health::{HealthRegistry, HealthSnapshot, HealthState, InFlight, TargetHealth, now_ms};
pub use latency::{
    LatencyBook, LatencyStats, MAX_SAMPLE_AGE_MS, RING_CAPACITY, Rank, RankedItem, Sample,
    SampleSource, Tier, TierThresholds, order_by_latency,
};
pub use name::{
    NameError, ParsedName, RESERVED_PROVIDER_SLUG, parse_name, validate_alias, validate_slug,
};
pub use select::{Candidates, SelectRequest, account_model_key, select};
pub use session::SessionAffinity;
pub use table::{
    ListingKind, ModelListing, Resolution, ResolutionKind, ResolveError, RouteTable,
    RouteTableHandle,
};
pub use target::{Prices, RouteTarget, TargetId};
