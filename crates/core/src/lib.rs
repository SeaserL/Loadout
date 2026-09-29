// SPDX-License-Identifier: Apache-2.0

//! Loadout Core: runtime assembly, configuration, control plane, task supervision
//! (ADR-007) and coordinated graceful shutdown (ADR-002).

mod control;
mod daemon;
mod dispatcher;
mod error;
pub mod logging;
mod paths;
mod route_build;
pub mod settings;
mod sinks;
pub mod supervisor;
mod usage;

pub use daemon::{DaemonOptions, run};
pub use error::DaemonError;
pub use paths::DataPaths;

/// Version of this Loadout build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
