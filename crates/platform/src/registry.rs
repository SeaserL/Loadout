// SPDX-License-Identifier: Apache-2.0

//! Read-only registry lookups.

#![allow(unsafe_code)]

use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};

use crate::PlatformError;
use crate::wide::{from_wide, to_wide};

fn read_string(
    root: HKEY,
    key: &str,
    value: Option<&str>,
) -> Result<Option<String>, PlatformError> {
    let key = to_wide(key);
    let name = value.map(to_wide);
    let name_ptr = name.as_ref().map_or(ptr::null(), |n| n.as_ptr());
    let mut buf: Vec<u16> = vec![0; 260];
    loop {
        let mut bytes = u32::try_from(buf.len() * 2)
            .map_err(|_| PlatformError::InvalidData("value too long".into()))?;
        // SAFETY: strings are NUL-terminated (or null for the default value) and `buf` holds
        // `bytes` bytes.
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                name_ptr,
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        match status {
            ERROR_SUCCESS => {
                buf.truncate(bytes as usize / 2);
                return Ok(Some(from_wide(&buf)));
            }
            ERROR_FILE_NOT_FOUND => return Ok(None),
            ERROR_MORE_DATA => buf.resize((bytes as usize).div_ceil(2) + 1, 0),
            code => return Err(PlatformError::os("RegGetValueW", code)),
        }
    }
}

/// The registered location of an application (`App Paths\<exe>`), per user first, then per
/// machine.
pub fn app_path(exe: &str) -> Option<PathBuf> {
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .find_map(|root| read_string(root, &key, None).ok().flatten())
        .map(|p| PathBuf::from(p.trim_matches('"')))
        .filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_apps_have_no_path() {
        assert_eq!(app_path("loadout-no-such-app.exe"), None);
    }
}
