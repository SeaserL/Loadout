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

    #[test]
    fn restricted_dir_grants_only_the_user_and_system() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inner = dir.path().join("profile");
        std::fs::create_dir(&inner).expect("mkdir");
        restrict_dir_to_current_user(&inner).expect("restrict");
        let sddl = dacl_sddl(&inner).expect("sddl");
        let sid = current_user_sid().expect("sid");
        assert!(sddl.starts_with("D:P"), "protected: {sddl}");
        let aces: Vec<&str> = sddl
            .trim_start_matches("D:P")
            .split(')')
            .filter(|a| !a.is_empty())
            .collect();
        assert_eq!(aces.len(), 2, "{sddl}");
        assert!(aces.iter().any(|a| a.ends_with(&format!(";{sid}"))));
        assert!(aces.iter().any(|a| a.ends_with(";SY")));

        // New files inherit the restricted DACL and stay writable for the user.
        let file = inner.join("Cookies");
        std::fs::write(&file, b"x").expect("write");
        assert!(dacl_sddl(&file).expect("file sddl").contains(&sid));
    }
}
