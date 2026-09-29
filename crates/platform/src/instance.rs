// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::PlatformError;
use crate::wide::{last_error, to_wide};

/// Holds a named mutex for the lifetime of the process to guarantee a single daemon per user
/// (ADR-002). The OS releases it automatically if the process dies.
#[derive(Debug)]
pub struct InstanceLock {
    handle: HANDLE,
    name: String,
}

// SAFETY: a mutex handle is a kernel object reference usable from any thread; we never
// wait on or release the mutex, only close the handle once on drop.
unsafe impl Send for InstanceLock {}
// SAFETY: see `Send`; no method mutates through `&self`.
unsafe impl Sync for InstanceLock {}

impl InstanceLock {
    /// Name used by the daemon: `Local\Loadout-<suffix>`, where `suffix` is normally the user
    /// SID (tests pass a unique value).
    pub fn name_for(suffix: &str) -> String {
        format!(r"Local\Loadout-{suffix}")
    }

    /// Returns `Ok(None)` if another holder already owns `name`.
    pub fn acquire(name: &str) -> Result<Option<Self>, PlatformError> {
        let wide = to_wide(name);
        // SAFETY: `wide` is NUL-terminated; null security attributes use the default DACL.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, wide.as_ptr()) };
        let code = last_error();
        if handle.is_null() {
            return Err(PlatformError::os("CreateMutexW", code));
        }
        if code == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` is a valid handle we own and have not closed.
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self {
            handle,
            name: name.to_owned(),
        }))
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and closed exactly once.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_until_first_is_dropped() {
        let name = InstanceLock::name_for(&format!("test-{}", uuid::Uuid::new_v4()));

        let first = InstanceLock::acquire(&name)
            .expect("acquire")
            .expect("first holder");
        assert_eq!(first.name(), name);
        assert!(InstanceLock::acquire(&name).expect("acquire").is_none());

        drop(first);
        assert!(InstanceLock::acquire(&name).expect("acquire").is_some());
    }
}
