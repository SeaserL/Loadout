// SPDX-License-Identifier: Apache-2.0

use crate::UpstreamFailure;

#[derive(Clone, Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0} is not supported by this provider")]
    NotSupported(&'static str),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("an API key is required")]
    MissingKey,

    #[error("upstream request failed: {0}")]
    Upstream(UpstreamFailure),

    #[error("unexpected upstream response: {0}")]
    InvalidResponse(String),
}
