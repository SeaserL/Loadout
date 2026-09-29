// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::PlatformError;
use crate::wide::{from_wide_ptr, last_error};

/// String form (`S-1-5-21-...`) of the SID of the user running this process.
pub fn current_user_sid() -> Result<String, PlatformError> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(PlatformError::os("OpenProcessToken", last_error()));
    }
    let token = TokenHandle(token);

    let mut len = 0u32;
    // SAFETY: size query with a null buffer; failure with ERROR_INSUFFICIENT_BUFFER is expected.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(PlatformError::os("GetTokenInformation", last_error()));
    }

    // u64 storage keeps TOKEN_USER (which contains pointers) correctly aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `len` bytes.
    if unsafe { GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return Err(PlatformError::os("GetTokenInformation", last_error()));
    }

    // SAFETY: the call above filled `buf` with a TOKEN_USER whose SID points into `buf`.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = ptr::null_mut();
    // SAFETY: `sid` is valid while `buf` lives; `text` is a valid out-pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(PlatformError::os("ConvertSidToStringSidW", last_error()));
    }
    // SAFETY: on success `text` is a NUL-terminated string allocated with LocalAlloc,
    // which we free exactly once after copying it.
    let sid = unsafe {
        let s = from_wide_ptr(text);
        LocalFree(text.cast());
        s
    };
    Ok(sid)
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcessToken and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid_has_expected_shape() {
        let sid = current_user_sid().expect("sid");
        assert!(sid.starts_with("S-1-"), "unexpected SID {sid}");
        assert_eq!(sid, current_user_sid().expect("sid again"));
    }
}
