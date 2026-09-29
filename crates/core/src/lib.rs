// SPDX-License-Identifier: Apache-2.0

//! Loadout Core: runtime assembly, configuration, task supervision (ADR-007) and
//! coordinated graceful shutdown (ADR-002).

mod daemon;
mod dispatcher;
mod error;
pub mod logging;
mod paths;
pub mod settings;
pub mod supervisor;

pub use daemon::{DaemonOptions, run};
pub use error::DaemonError;
pub use paths::DataPaths;

/// Version of this Loadout build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
