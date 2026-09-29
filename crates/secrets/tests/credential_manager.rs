// SPDX-License-Identifier: Apache-2.0

//! Exercises the real Windows Credential Manager. Every entry uses the `loadout-test:` prefix
//! and is removed by a drop guard, even when an assertion fails.

#![cfg(windows)]

use loadout_secrets::{CredentialManagerStore, Secret, SecretRef, SecretStore, SecretStoreError};

const TEST_PREFIX: &str = "loadout-test:";

struct Cleanup<'a> {
    store: &'a CredentialManagerStore,
    secret_ref: SecretRef,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = self.store.delete(&self.secret_ref);
    }
}

fn store() -> CredentialManagerStore {
    CredentialManagerStore::with_prefix(TEST_PREFIX)
}

#[test]
fn put_get_overwrite_delete() {
    let store = store();
    let r = SecretRef::generate();
    let _cleanup = Cleanup {
        store: &store,
        secret_ref: r,
    };

    assert!(!store.exists(&r).expect("exists before put"));

    store.put(&r, &Secret::new("sk-test-first")).expect("put");
    assert!(store.exists(&r).expect("exists after put"));
    assert_eq!(store.get(&r).expect("get").expose(), "sk-test-first");

    store
        .put(&r, &Secret::new("sk-test-second"))
        .expect("overwrite");
    assert_eq!(store.get(&r).expect("get").expose(), "sk-test-second");

    assert!(store.delete(&r).expect("delete"));
    assert!(!store.exists(&r).expect("exists after delete"));
}

#[test]
fn missing_secret_is_not_found() {
    let store = store();
    let r = SecretRef::generate();

    match store.get(&r) {
        Err(SecretStoreError::NotFound(missing)) => assert_eq!(missing, r),
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(!store.delete(&r).expect("delete missing"));
}

#[test]
fn unicode_and_empty_values_roundtrip() {
    let store = store();
    for value in ["", "密钥-κλειδί-🔑", "line1\nline2"] {
        let r = SecretRef::generate();
        let _cleanup = Cleanup {
            store: &store,
            secret_ref: r,
        };
        store.put(&r, &Secret::new(value)).expect("put");
        assert_eq!(store.get(&r).expect("get").expose(), value);
    }
}

#[test]
fn oversized_secret_is_rejected_without_writing() {
    let store = store();
    let r = SecretRef::generate();
    let _cleanup = Cleanup {
        store: &store,
        secret_ref: r,
    };

    let big = "x".repeat(2561);
    match store.put(&r, &Secret::new(big)) {
        Err(SecretStoreError::TooLarge { len, max }) => {
            assert_eq!(len, 2561);
            assert_eq!(max, 2560);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
    assert!(!store.exists(&r).expect("exists"));

    let max = "y".repeat(2560);
    store
        .put(&r, &Secret::new(max.clone()))
        .expect("put max size");
    assert_eq!(store.get(&r).expect("get").expose(), max);
}

#[test]
fn prefixes_isolate_entries() {
    let test_store = store();
    let other_store = CredentialManagerStore::with_prefix("loadout-test-other:");
    let r = SecretRef::generate();
    let _cleanup = Cleanup {
        store: &test_store,
        secret_ref: r,
    };

    test_store.put(&r, &Secret::new("value")).expect("put");
    assert!(!other_store.exists(&r).expect("other prefix exists"));
    assert_eq!(test_store.target_name(&r), format!("{TEST_PREFIX}{r}"));
}

/// Regression: without in-process serialization, concurrent writes made deleted entries
/// reappear (ExecPlan 001 Stage 1).
#[test]
fn concurrent_writes_do_not_resurrect_deletes() {
    let store = store();
    let refs: Vec<Vec<SecretRef>> = (0..8)
        .map(|_| (0..20).map(|_| SecretRef::generate()).collect())
        .collect();

    std::thread::scope(|scope| {
        for thread_refs in &refs {
            let store = &store;
            scope.spawn(move || {
                for r in thread_refs {
                    store.put(r, &Secret::new("concurrent")).expect("put");
                    assert!(store.delete(r).expect("delete"));
                }
            });
        }
    });

    let survivors: Vec<_> = refs
        .iter()
        .flatten()
        .filter(|r| store.exists(r).expect("exists"))
        .collect();
    for r in &survivors {
        let _ = store.delete(r);
    }
    assert!(
        survivors.is_empty(),
        "{} deleted entries reappeared",
        survivors.len()
    );
}

#[test]
fn errors_never_contain_secret_material() {
    let store = store();
    let r = SecretRef::generate();
    let secret_text = "sk-should-never-appear-".repeat(200);

    let err = store
        .put(&r, &Secret::new(secret_text.clone()))
        .expect_err("too large");
    let rendered = format!("{err} {err:?}");
    assert!(!rendered.contains("sk-should-never-appear"));

    let missing = store.get(&r).expect_err("missing");
    assert!(!format!("{missing} {missing:?}").contains("sk-should-never-appear"));
}
