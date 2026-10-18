// SPDX-License-Identifier: Apache-2.0

//! Secret wrapper types and the OS-backed secret store.
//!
//! Raw secrets never enter SQLite or logs; only a [`SecretRef`] handle is persisted
//! (ADR-004). The Windows implementation stores secrets in Credential Manager.

mod memory;
mod redact;
mod secret;
mod store;
#[cfg(windows)]
mod windows;

pub use memory::InMemorySecretStore;
pub use redact::{redact, redact_str};
pub use secret::{Secret, SecretRef, SecretRefParseError};
pub use store::{SecretStore, SecretStoreError};
#[cfg(windows)]
pub use windows::CredentialManagerStore;
#[cfg(windows)]
pub use windows::github_git_credential;
