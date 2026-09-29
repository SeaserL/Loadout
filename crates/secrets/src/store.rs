// SPDX-License-Identifier: Apache-2.0

use crate::{Secret, SecretRef};

/// Errors from a [`SecretStore`]. Never carries secret material.
#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("secret {0} not found")]
    NotFound(SecretRef),

    #[error("secret is {len} bytes; the store accepts at most {max} bytes")]
    TooLarge { len: usize, max: usize },

    #[error("stored secret {0} is not valid UTF-8")]
    Corrupt(SecretRef),

    #[error("{operation} failed with OS error {code}")]
    Os { operation: &'static str, code: u32 },
}

/// Storage for secret values, addressed by [`SecretRef`] (ADR-004).
///
/// Operations are synchronous: the OS store is local and fast, and it is only used from
/// control-plane code, never on the request hot path.
pub trait SecretStore: Send + Sync {
    /// Creates or overwrites the secret for `secret_ref`.
    fn put(&self, secret_ref: &SecretRef, secret: &Secret) -> Result<(), SecretStoreError>;

    /// Reads the secret. Returns [`SecretStoreError::NotFound`] if absent.
    fn get(&self, secret_ref: &SecretRef) -> Result<Secret, SecretStoreError>;

    /// Deletes the secret. Returns `false` if it did not exist.
    fn delete(&self, secret_ref: &SecretRef) -> Result<bool, SecretStoreError>;

    fn exists(&self, secret_ref: &SecretRef) -> Result<bool, SecretStoreError> {
        match self.get(secret_ref) {
            Ok(_) => Ok(true),
            Err(SecretStoreError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
