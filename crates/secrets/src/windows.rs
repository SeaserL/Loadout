// SPDX-License-Identifier: Apache-2.0

//! Windows Credential Manager backend (ADR-004).
//!
//! Each secret is a `CRED_TYPE_GENERIC` credential named `<prefix><uuid>`, persisted with
//! `CRED_PERSIST_LOCAL_MACHINE` so it stays on this machine and does not roam with the
//! user profile. The blob is the UTF-8 bytes of the secret.
//!
//! All Credential Manager calls are serialized through a process-wide lock: concurrent
//! writes from multiple threads were observed to lose deletes (a deleted entry reappears
//! after a racing write from another thread persists), see ExecPlan 001 Stage 1.

#![allow(unsafe_code)]

use std::iter;
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard};

use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredFree, CredReadW, CredWriteW,
};

use crate::{Secret, SecretRef, SecretStore, SecretStoreError};

const DEFAULT_PREFIX: &str = "loadout:";
const USER_NAME: &str = "loadout";
const MAX_BLOB: usize = CRED_MAX_CREDENTIAL_BLOB_SIZE as usize;

static CREDENTIAL_LOCK: Mutex<()> = Mutex::new(());

/// Read the existing default GitHub Git credential without prompting or modifying it.
/// Only used by the fixed-repository release updater; never enumerate other credentials.
pub fn github_git_credential() -> Option<Secret> {
    let target = to_wide("git:https://github.com");
    let mut raw = ptr::null_mut();
    let _guard = credential_lock();
    // SAFETY: target is terminated and raw is a valid out-pointer. OwnedCredential frees it.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
        return None;
    }
    let credential = OwnedCredential(raw);
    decode_git_credential(credential.blob())
}

fn decode_git_credential(blob: &[u8]) -> Option<Secret> {
    // GCM versions use UTF-16LE or UTF-8. GitHub tokens contain ASCII printable bytes.
    let bytes: Vec<u8> = if blob.len().is_multiple_of(2)
        && blob.as_chunks::<2>().0.iter().all(|pair| pair[1] == 0)
    {
        blob.as_chunks::<2>().0.iter().map(|pair| pair[0]).collect()
    } else {
        blob.to_vec()
    };
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Some(Secret::new(String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod git_credential_tests {
    use super::decode_git_credential;
    #[test]
    fn accepts_synthetic_encodings_and_rejects_controls() {
        assert_eq!(
            decode_git_credential(b"synthetic")
                .expect("synthetic credential")
                .expose(),
            "synthetic"
        );
        assert_eq!(
            decode_git_credential(b"s\0y\0n\0")
                .expect("synthetic credential")
                .expose(),
            "syn"
        );
        assert!(decode_git_credential(b"bad\nheader").is_none());
        assert!(decode_git_credential(b"").is_none());
    }
}

fn credential_lock() -> MutexGuard<'static, ()> {
    // The lock guards no data, so a poisoned lock is safe to reuse.
    CREDENTIAL_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// [`SecretStore`] backed by the current user's Windows Credential Manager.
#[derive(Debug, Clone)]
pub struct CredentialManagerStore {
    prefix: String,
}

impl CredentialManagerStore {
    /// Store using the production `loadout:` target-name prefix.
    pub fn new() -> Self {
        Self::with_prefix(DEFAULT_PREFIX)
    }

    /// Store using a custom prefix, so tests never touch production entries.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    /// The Credential Manager target name for `secret_ref`.
    pub fn target_name(&self, secret_ref: &SecretRef) -> String {
        format!("{}{}", self.prefix, secret_ref)
    }

    fn wide_target(&self, secret_ref: &SecretRef) -> Vec<u16> {
        to_wide(&self.target_name(secret_ref))
    }
}

impl Default for CredentialManagerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for CredentialManagerStore {
    fn put(&self, secret_ref: &SecretRef, secret: &Secret) -> Result<(), SecretStoreError> {
        let blob = secret.expose().as_bytes();
        if blob.len() > MAX_BLOB {
            return Err(SecretStoreError::TooLarge {
                len: blob.len(),
                max: MAX_BLOB,
            });
        }

        let mut target = self.wide_target(secret_ref);
        let mut user = to_wide(USER_NAME);
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            // Bounded by MAX_BLOB above, so the cast cannot truncate.
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..Default::default()
        };

        let _guard = credential_lock();
        // SAFETY: every pointer in `credential` refers to a live, NUL-terminated (strings) or
        // correctly sized (blob) buffer that outlives the call. CredWriteW only reads the blob.
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            return Err(last_os_error("CredWriteW"));
        }
        Ok(())
    }

    fn get(&self, secret_ref: &SecretRef) -> Result<Secret, SecretStoreError> {
        let target = self.wide_target(secret_ref);
        let mut raw: *mut CREDENTIALW = ptr::null_mut();

        let _guard = credential_lock();
        // SAFETY: `target` is NUL-terminated and `raw` is a valid out-pointer.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
        if ok == 0 {
            return Err(match last_error_code() {
                ERROR_NOT_FOUND => SecretStoreError::NotFound(*secret_ref),
                code => SecretStoreError::Os {
                    operation: "CredReadW",
                    code,
                },
            });
        }

        let credential = OwnedCredential(raw);
        let text = std::str::from_utf8(credential.blob())
            .map_err(|_| SecretStoreError::Corrupt(*secret_ref))?;
        Ok(Secret::new(text))
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<bool, SecretStoreError> {
        let target = self.wide_target(secret_ref);

        let _guard = credential_lock();
        // SAFETY: `target` is NUL-terminated and outlives the call.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            return match last_error_code() {
                ERROR_NOT_FOUND => Ok(false),
                code => Err(SecretStoreError::Os {
                    operation: "CredDeleteW",
                    code,
                }),
            };
        }
        Ok(true)
    }
}

/// A credential returned by `CredReadW`; zeroes the blob and frees it on drop.
struct OwnedCredential(*mut CREDENTIALW);

impl OwnedCredential {
    fn blob(&self) -> &[u8] {
        // SAFETY: CredReadW succeeded, so `self.0` points to a valid CREDENTIALW whose blob
        // pointer/size pair describes memory owned by that allocation until CredFree.
        unsafe {
            let credential = &*self.0;
            if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                &[]
            } else {
                slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            }
        }
    }
}

impl Drop for OwnedCredential {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful CredReadW and is freed exactly once here.
        // The blob lies inside that allocation, so zeroing it before CredFree is valid.
        unsafe {
            let credential = &*self.0;
            if !credential.CredentialBlob.is_null() {
                ptr::write_bytes(
                    credential.CredentialBlob,
                    0,
                    credential.CredentialBlobSize as usize,
                );
            }
            CredFree(self.0.cast());
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(iter::once(0)).collect()
}

fn last_error_code() -> u32 {
    // SAFETY: GetLastError has no preconditions.
    unsafe { GetLastError() }
}

fn last_os_error(operation: &'static str) -> SecretStoreError {
    SecretStoreError::Os {
        operation,
        code: last_error_code(),
    }
}
