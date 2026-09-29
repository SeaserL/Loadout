// SPDX-License-Identifier: Apache-2.0

//! Platform services (ADR-001). v1 ships only the Windows implementation: data directories,
//! per-user single-instance mutex, HKCU Run autostart and user SID lookup.
//!
//! This crate is the platform boundary: everything OS-specific that the rest of Loadout needs
//! goes through its public API, so another OS only has to provide the same functions.

#[cfg(not(windows))]
compile_error!("Loadout v1 supports Windows only (ADR-001)");

mod autostart;
mod console;
mod error;
mod instance;
mod paths;
mod process;
mod sid;
mod wide;

pub use autostart::{Autostart, AutostartStatus};
pub use console::{attach_parent_console, read_hidden_line};
pub use error::PlatformError;
pub use instance::InstanceLock;
pub use paths::default_data_dir;
pub use process::{DetachedProcess, spawn_detached};
pub use sid::current_user_sid;
