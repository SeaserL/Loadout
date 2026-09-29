// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// The client request is malformed.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The request uses a feature the target protocol cannot express.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// The upstream sent something we cannot parse.
    #[error("invalid upstream data: {0}")]
    InvalidUpstream(String),
}

impl ProtocolError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidRequest(msg.into())
    }

    pub(crate) fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }
}
