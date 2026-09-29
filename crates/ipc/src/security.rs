// SPDX-License-Identifier: Apache-2.0

//! Security descriptor restricting the pipe to the current user (ADR-002).

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::iter;
use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

/// SDDL granting full access to `sid` and LocalSystem only; `P` blocks inherited ACEs.
pub(crate) fn pipe_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)")
}

/// Owned security attributes for creating pipe instances.
pub(crate) struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

// SAFETY: the descriptor is an immutable heap allocation owned by this struct; it is only
// read by CreateNamedPipeW and freed once on drop.
unsafe impl Send for PipeSecurity {}
// SAFETY: see `Send`.
unsafe impl Sync for PipeSecurity {}

impl std::fmt::Debug for PipeSecurity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipeSecurity").finish_non_exhaustive()
    }
}

impl PipeSecurity {
    pub(crate) fn for_user(sid: &str) -> io::Result<Self> {
        if !sid.starts_with("S-1-") || !sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid SID"));
        }
        let sddl: Vec<u16> = pipe_sddl(sid).encode_utf16().chain(iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `descriptor` is a valid out-pointer.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    /// Pointer for `ServerOptions::create_with_security_attributes_raw`; valid while `self`
    /// lives.
    pub(crate) fn as_raw(&self) -> *mut c_void {
        ptr::from_ref(&self.attributes).cast_mut().cast()
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW with
        // LocalAlloc and freed exactly once.
        unsafe { LocalFree(self.descriptor) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sddl_grants_only_user_and_system() {
        assert_eq!(
            pipe_sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)(A;;GA;;;SY)"
        );
    }

    #[test]
    fn rejects_malformed_sid() {
        assert!(PipeSecurity::for_user("S-1-5-21)(A;;GA;;;WD").is_err());
        assert!(PipeSecurity::for_user("everyone").is_err());
    }

    #[test]
    fn builds_descriptor_for_current_user() {
        let sid = loadout_platform::current_user_sid().expect("sid");
        let security = PipeSecurity::for_user(&sid).expect("descriptor");
        assert!(!security.as_raw().is_null());
    }
}
