// SPDX-License-Identifier: Apache-2.0

//! Directory access control (ExecPlan 017 §5.4): browser profile directories hold signed-in
//! session cookies, so only the current user and SYSTEM may open them.

#![allow(unsafe_code)]

use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};

use crate::PlatformError;
use crate::sid::current_user_sid;
use crate::wide::{from_wide_ptr, last_error, to_wide};

fn wide_path(path: &Path) -> Result<Vec<u16>, PlatformError> {
    let s = path
        .to_str()
        .ok_or_else(|| PlatformError::InvalidData("path is not valid Unicode".into()))?;
    Ok(to_wide(s))
}

/// Replaces the DACL of `dir` with a protected one granting full control to the current
/// user and SYSTEM only (inherited by everything created inside).
pub fn restrict_dir_to_current_user(dir: &Path) -> Result<(), PlatformError> {
    let sid = current_user_sid()?;
    let sddl = to_wide(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"));
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd descriptor.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(PlatformError::os(
            "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            last_error(),
        ));
    }
    struct Local(PSECURITY_DESCRIPTOR);
    impl Drop for Local {
        fn drop(&mut self) {
            // SAFETY: allocated by the Convert* call, freed once.
            unsafe { LocalFree(self.0) };
        }
    }
    let sd = Local(sd);
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl: *mut ACL = ptr::null_mut();
    // SAFETY: `sd.0` is a valid descriptor; out-pointers are valid.
    if unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) } == 0 {
        return Err(PlatformError::os("GetSecurityDescriptorDacl", last_error()));
    }
    let path = wide_path(dir)?;
    // SAFETY: `path` is NUL-terminated; `dacl` points into `sd`, alive for the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(PlatformError::os("SetNamedSecurityInfoW", status));
    }
    Ok(())
}

/// The DACL of `path` in SDDL form (tests and diagnostics).
pub fn dacl_sddl(path: &Path) -> Result<String, PlatformError> {
    let wide = wide_path(path)?;
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated; only the descriptor out-pointer is requested.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut sd,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(PlatformError::os("GetNamedSecurityInfoW", status));
    }
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sd` is a valid descriptor; `text` receives a LocalAlloc'd string.
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    };
    let result = if ok == 0 {
        Err(PlatformError::os(
            "ConvertSecurityDescriptorToStringSecurityDescriptorW",
            last_error(),
        ))
    } else {
        // SAFETY: a NUL-terminated string returned by the call above.
        Ok(unsafe { from_wide_ptr(text) })
    };
    // SAFETY: both allocated by the calls above (null is accepted), freed once.
    unsafe {
        LocalFree(text.cast());
        LocalFree(sd);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSidToSidW,
    };

    /// Full `S-1-...` form of an SDDL trustee, which may be an alias such as `SY`, or `LA`
    /// when the current user is the built-in Administrator (as on CI runners).
    fn canonical_sid(trustee: &str) -> String {
        let wide = to_wide(trustee);
        let mut sid = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; `sid` receives a LocalAlloc'd SID.
        assert_ne!(
            unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) },
            0,
            "trustee {trustee}"
        );
        let mut text = ptr::null_mut();
        // SAFETY: `sid` is valid until freed below; `text` is a valid out-pointer.
        let ok = unsafe { ConvertSidToStringSidW(sid, &mut text) };
        // SAFETY: both allocated by the calls above, freed once.
        let s = unsafe {
            let s = (ok != 0).then(|| from_wide_ptr(text));
            LocalFree(text.cast());
            LocalFree(sid);
            s
        };
        s.expect("ConvertSidToStringSidW")
    }

    /// Canonical trustee SIDs of the ACEs in a `D:...` SDDL string.
    fn trustees(sddl: &str) -> Vec<String> {
        sddl.split(')')
            .filter_map(|ace| ace.split_once('('))
            .filter_map(|(_, body)| body.rsplit(';').next())
            .map(canonical_sid)
            .collect()
    }

    #[test]
    fn restricted_dir_grants_only_the_user_and_system() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inner = dir.path().join("profile");
        std::fs::create_dir(&inner).expect("mkdir");
        restrict_dir_to_current_user(&inner).expect("restrict");
        let sddl = dacl_sddl(&inner).expect("sddl");
        let sid = current_user_sid().expect("sid");
        let system = canonical_sid("SY");
        assert!(sddl.starts_with("D:P"), "protected: {sddl}");
        let got = trustees(&sddl);
        assert_eq!(got.len(), 2, "{sddl}");
        assert!(got.contains(&sid), "user {sid} in {sddl}");
        assert!(got.contains(&system), "SYSTEM in {sddl}");

        // New files inherit the restricted DACL and stay writable for the user.
        let file = inner.join("Cookies");
        std::fs::write(&file, b"x").expect("write");
        let file_sddl = dacl_sddl(&file).expect("file sddl");
        assert!(trustees(&file_sddl).contains(&sid), "{file_sddl}");
    }
}
