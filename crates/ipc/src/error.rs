// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Error codes sent by the daemon. Unknown codes from a newer daemon map to `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    VersionMismatch,
    NotFound,
    Conflict,
    Internal,
    #[serde(other)]
    Unknown,
}

/// Error payload of a response frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

/// Client-side IPC failure.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("the Loadout daemon is not running")]
    NotRunning,

    #[error("IPC I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("IPC protocol error: {0}")]
    Protocol(String),

    #[error("daemon speaks IPC protocol {daemon}, this client speaks {client}")]
    VersionMismatch { daemon: String, client: u32 },

    #[error("connection closed by the daemon")]
    Closed,

    #[error(transparent)]
    Remote(#[from] ErrorBody),
}
