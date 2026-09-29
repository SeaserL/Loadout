// SPDX-License-Identifier: Apache-2.0

//! Local agent discovery (Harness §12): what is installed and logged in, from executables
//! on PATH and the presence / metadata of known config files. Never returns token values.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What a Discovery driver found on this machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovered {
    pub installed: bool,
    pub executable: Option<PathBuf>,
    pub logged_in: bool,
    /// How the tool authenticates, e.g. `oauth`, `chatgpt`, `api_key`.
    pub auth: Option<String>,
    /// Safe, non-secret detail such as a plan name.
    pub detail: Option<String>,
    /// The stored login has expired; running the official CLI refreshes it.
    pub expired: bool,
    pub config_path: Option<PathBuf>,
}

/// Finds `name` on PATH, trying the Windows executable extensions (`.exe`, `.cmd`, `.bat`,
/// …) so npm-installed CLIs (`codex.cmd`) are found too.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_in(name, &path)
}

pub(crate) fn find_in(name: &str, path: &OsString) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    for dir in std::env::split_paths(path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `%VAR%` override or `home/<default>`.
pub fn config_dir(env: &str, home: &Path, default: &str) -> PathBuf {
    std::env::var_os(env)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cmd_shims_on_a_custom_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("fakecli.cmd"), "@echo off").expect("write");
        let path = std::env::join_paths([dir.path()]).expect("join");
        assert_eq!(
            find_in("fakecli", &path),
            Some(dir.path().join("fakecli.cmd"))
        );
        assert_eq!(find_in("missing", &path), None);
    }
}
