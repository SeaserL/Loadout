// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{Secret, SecretRef, SecretStore, SecretStoreError};

/// Process-local [`SecretStore`] for tests of crates that depend on a store.
/// Not used by the daemon.
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    entries: Mutex<HashMap<SecretRef, Secret>>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored secrets (lets tests check that replaced secrets are deleted).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SecretRef, Secret>> {
        // A poisoned map is still structurally valid; recover it rather than propagate panics.
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for InMemorySecretStore {
    fn put(&self, secret_ref: &SecretRef, secret: &Secret) -> Result<(), SecretStoreError> {
        self.lock()
            .insert(*secret_ref, Secret::new(secret.expose()));
        Ok(())
    }

    fn get(&self, secret_ref: &SecretRef) -> Result<Secret, SecretStoreError> {
        self.lock()
            .get(secret_ref)
            .map(|s| Secret::new(s.expose()))
            .ok_or(SecretStoreError::NotFound(*secret_ref))
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<bool, SecretStoreError> {
        Ok(self.lock().remove(secret_ref).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_delete() {
        let store = InMemorySecretStore::new();
        let r = SecretRef::generate();

        assert!(!store.exists(&r).expect("exists"));
        store.put(&r, &Secret::new("one")).expect("put");
        assert_eq!(store.get(&r).expect("get").expose(), "one");

        store.put(&r, &Secret::new("two")).expect("overwrite");
        assert_eq!(store.get(&r).expect("get").expose(), "two");

        assert!(store.delete(&r).expect("delete"));
        assert!(!store.delete(&r).expect("delete again"));
        assert!(matches!(store.get(&r), Err(SecretStoreError::NotFound(_))));
    }
}
