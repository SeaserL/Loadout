// SPDX-License-Identifier: Apache-2.0

use loadout_platform::PlatformError;
use loadout_storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another Loadout daemon is already running for this user")]
    AlreadyRunning,

    #[error("port {0} is already in use; change it with `loadout config set gateway.port <port>`")]
    PortInUse(u16),

    #[error("invalid setting: {0}")]
    Config(String),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Platform(#[from] PlatformError),

    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: std::io::Error,
    },

    #[error("{0} stopped unexpectedly")]
    ServiceFailed(&'static str),
}

impl DaemonError {
    pub(crate) fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    /// Process exit code for `loadoutd`.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::AlreadyRunning => 2,
            Self::PortInUse(_) => 3,
            Self::Config(_) => 4,
            _ => 1,
        }
    }
}
