// SPDX-License-Identifier: Apache-2.0

//! Login autostart via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (ADR-002).

#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use crate::PlatformError;
use crate::wide::{from_wide, to_wide};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const DEFAULT_VALUE_NAME: &str = "Loadout";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutostartStatus {
    Disabled,
    Enabled {
        command: String,
        /// Whether the executable in the command still exists (it may have been moved).
        executable_exists: bool,
    },
}

/// Manages one value under the per-user Run key.
#[derive(Clone, Debug)]
pub struct Autostart {
    value_name: String,
}

impl Default for Autostart {
    fn default() -> Self {
        Self::new()
    }
}

impl Autostart {
    pub fn new() -> Self {
        Self::with_value_name(DEFAULT_VALUE_NAME)
    }

    /// Custom value name, so tests never touch the real `Loadout` entry.
    pub fn with_value_name(value_name: impl Into<String>) -> Self {
        Self {
            value_name: value_name.into(),
        }
    }

    /// Registers `"<executable>" <args...>` to run at login.
    pub fn enable(&self, executable: &Path, args: &[&str]) -> Result<(), PlatformError> {
        let exe = executable.to_str().ok_or_else(|| {
            PlatformError::InvalidData("executable path is not valid Unicode".into())
        })?;
        if exe.contains('"') {
            return Err(PlatformError::InvalidData(
                "executable path contains a quote".into(),
            ));
        }
        let mut command = format!("\"{exe}\"");
        for arg in args {
            command.push(' ');
            command.push_str(arg);
        }

        let key = to_wide(RUN_KEY);
        let name = to_wide(&self.value_name);
        let data = to_wide(&command);
        let bytes = u32::try_from(data.len() * 2)
            .map_err(|_| PlatformError::InvalidData("command too long".into()))?;
        // SAFETY: all strings are NUL-terminated; `data` holds `bytes` bytes including the NUL
        // that REG_SZ requires.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(PlatformError::os("RegSetKeyValueW", status));
        }
        Ok(())
    }

    /// Removes the entry. Returns `false` if it was not registered.
    pub fn disable(&self) -> Result<bool, PlatformError> {
        let key = to_wide(RUN_KEY);
        let name = to_wide(&self.value_name);
        // SAFETY: both strings are NUL-terminated.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        match status {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            code => Err(PlatformError::os("RegDeleteKeyValueW", code)),
        }
    }

    /// The registered command line, if any.
    pub fn command(&self) -> Result<Option<String>, PlatformError> {
        let key = to_wide(RUN_KEY);
        let name = to_wide(&self.value_name);
        let mut buf: Vec<u16> = vec![0; 260];
        loop {
            let mut bytes = u32::try_from(buf.len() * 2)
                .map_err(|_| PlatformError::InvalidData("value too long".into()))?;
            // SAFETY: strings are NUL-terminated and `buf` holds `bytes` bytes.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
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

    pub fn status(&self) -> Result<AutostartStatus, PlatformError> {
        Ok(match self.command()? {
            None => AutostartStatus::Disabled,
            Some(command) => {
                let executable_exists = executable_of(&command).is_some_and(|p| p.is_file());
                AutostartStatus::Enabled {
                    command,
                    executable_exists,
                }
            }
        })
    }
}

/// Extracts the executable from a Run command line (`"C:\x\y.exe" args` or `C:\y.exe args`).
fn executable_of(command: &str) -> Option<PathBuf> {
    let command = command.trim_start();
    let exe = if let Some(rest) = command.strip_prefix('"') {
        &rest[..rest.find('"')?]
    } else {
        command.split_whitespace().next()?
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Cleanup(Autostart);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.disable();
        }
    }

    #[test]
    fn parses_executable_from_command() {
        assert_eq!(
            executable_of(r#""C:\Program Files\Loadout\loadoutd.exe" --background"#),
            Some(PathBuf::from(r"C:\Program Files\Loadout\loadoutd.exe"))
        );
        assert_eq!(
            executable_of(r"C:\bin\loadoutd.exe --background"),
            Some(PathBuf::from(r"C:\bin\loadoutd.exe"))
        );
        assert_eq!(executable_of(r#""unterminated"#), None);
        assert_eq!(executable_of(""), None);
    }

    #[test]
    fn enable_status_disable_roundtrip() {
        let autostart =
            Autostart::with_value_name(format!("Loadout-Test-{}", uuid::Uuid::new_v4()));
        let _cleanup = Cleanup(autostart.clone());
        let exe = std::env::current_exe().expect("current exe");

        assert_eq!(
            autostart.status().expect("status"),
            AutostartStatus::Disabled
        );

        autostart.enable(&exe, &["--background"]).expect("enable");
        let expected = format!("\"{}\" --background", exe.display());
        assert_eq!(
            autostart.status().expect("status"),
            AutostartStatus::Enabled {
                command: expected,
                executable_exists: true,
            }
        );

        assert!(autostart.disable().expect("disable"));
        assert!(!autostart.disable().expect("disable again"));
        assert_eq!(
            autostart.status().expect("status"),
            AutostartStatus::Disabled
        );
    }

    #[test]
    fn missing_executable_is_reported() {
        let autostart =
            Autostart::with_value_name(format!("Loadout-Test-{}", uuid::Uuid::new_v4()));
        let _cleanup = Cleanup(autostart.clone());

        autostart
            .enable(Path::new(r"C:\definitely\not\here\loadoutd.exe"), &[])
            .expect("enable");
        match autostart.status().expect("status") {
            AutostartStatus::Enabled {
                executable_exists, ..
            } => assert!(!executable_exists),
            AutostartStatus::Disabled => panic!("expected enabled"),
        }
    }
}
