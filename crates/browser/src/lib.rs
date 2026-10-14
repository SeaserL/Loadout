// SPDX-License-Identifier: Apache-2.0

//! Browser-session transport (ADR-014, ExecPlan 017).
//!
//! Each opted-in account gets its own Microsoft Edge process with a dedicated profile,
//! started inside a daemon-owned job and driven over a private CDP pipe. The browser is
//! unmodified and presents itself as it normally would: nothing here hides automation,
//! spoofs fingerprints, solves challenges or reads cookies (ADR-014 §2).

mod cdp;
mod edge;
mod launch;
mod page;
mod plan;
mod session;

pub use cdp::{ALLOWED_METHODS, Cdp, Event, Subscription, is_allowed};
pub use edge::{EdgeInstall, MIN_MAJOR_VERSION, find_edge};
pub use launch::{Browser, FORBIDDEN_ARG_PREFIXES, LaunchOptions, launch_args};
pub use page::Page;
pub use plan::{ConversationMode, flatten};
pub use session::{AccountEvents, AccountSettings, Launcher, WebAccount, WebAccountStatus};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrowserError {
    #[error("Microsoft Edge is not installed")]
    EdgeNotFound,
    #[error("Microsoft Edge {0} is too old; update Edge")]
    EdgeTooOld(String),
    #[error("cannot start the browser: {0}")]
    Launch(String),
    #[error("the browser profile is in use by another Edge window; close it or restart the daemon")]
    ProfileInUse,
    #[error("the browser connection is closed")]
    Closed,
    #[error("browser method {0} is not allowed")]
    NotAllowed(&'static str),
    #[error("browser protocol error: {0}")]
    Protocol(String),
    #[error("timed out: {0}")]
    Timeout(&'static str),
    #[error("navigation failed: {0}")]
    Navigation(String),
    #[error("file system error: {0}")]
    Io(String),
    #[error("{0} is not a browser-session provider")]
    NotWebProvider(String),
}

/// Removes a profile directory, retrying while a just-closed browser still holds files.
pub fn remove_profile_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let mut last = Ok(());
    for _ in 0..20 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Err(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    last
}
