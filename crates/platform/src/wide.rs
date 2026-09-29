// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::iter;

use windows_sys::Win32::Foundation::GetLastError;

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(iter::once(0)).collect()
}

/// Converts a buffer that may contain a trailing NUL into a `String`.
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Reads a NUL-terminated UTF-16 string.
///
/// # Safety
/// `ptr` must be non-null and point to a NUL-terminated UTF-16 string.
pub(crate) unsafe fn from_wide_ptr(ptr: *const u16) -> String {
    let mut len = 0;
    // SAFETY: guaranteed NUL-terminated by the caller.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

pub(crate) fn last_error() -> u32 {
    // SAFETY: GetLastError has no preconditions.
    unsafe { GetLastError() }
}
