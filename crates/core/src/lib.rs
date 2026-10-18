// SPDX-License-Identifier: Apache-2.0

//! Loadout Core: runtime assembly, configuration, control plane, task supervision
//! (ADR-007) and coordinated graceful shutdown (ADR-002).

mod backup;
mod bench;
mod check;
mod clients;
mod control;
mod daemon;
mod discovery;
mod dispatcher;
mod endpoints;
mod error;
mod keys_export;
mod latency;
pub mod logging;
mod notify;
mod oauth;
mod paths;
mod prices;
mod route_build;
pub mod settings;
mod sinks;
mod subpool;
pub mod supervisor;
mod unavailable;
mod updates;
mod usage;
mod web_session;

pub use daemon::{DaemonOptions, run};
pub use error::DaemonError;
pub use paths::DataPaths;

/// Version of this Loadout build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
