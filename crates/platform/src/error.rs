// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("{operation} failed with OS error {code}")]
    Os { operation: &'static str, code: u32 },

    #[error("environment variable {0} is not set")]
    MissingEnv(&'static str),

    #[error("invalid data: {0}")]
    InvalidData(String),
}

impl PlatformError {
    pub(crate) fn os(operation: &'static str, code: u32) -> Self {
        Self::Os { operation, code }
    }
}
