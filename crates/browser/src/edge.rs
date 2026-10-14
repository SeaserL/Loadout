// SPDX-License-Identifier: Apache-2.0

//! Locating Microsoft Edge (ExecPlan 017 §5.1) without starting it.

use std::path::{Path, PathBuf};

use crate::BrowserError;

/// Oldest major version tested with the pipe transport and `Network.streamResourceContent`.
pub const MIN_MAJOR_VERSION: u32 = 126;

/// An installed Edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeInstall {
    pub path: PathBuf,
    /// `major.minor.build.patch`, from the version directory next to the executable.
    pub version: Option<String>,
}

impl EdgeInstall {
    pub fn major(&self) -> Option<u32> {
        self.version.as_deref()?.split('.').next()?.parse().ok()
    }
}

/// The highest `a.b.c.d` directory name next to `exe` (Edge keeps one per installed version).
fn version_next_to(exe: &Path) -> Option<String> {
    let dir = exe.parent()?;
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let parts: Option<Vec<u32>> = name.split('.').map(|p| p.parse().ok()).collect();
            parts.filter(|p| p.len() == 4).map(|p| (p, name))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, name)| name)
}

/// The registered Edge, else the default install locations.
pub fn find_edge() -> Result<EdgeInstall, BrowserError> {
    let candidates = loadout_platform::app_path("msedge.exe").into_iter().chain(
        ["ProgramFiles(x86)", "ProgramFiles"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|base| {
                Path::new(&base)
                    .join("Microsoft")
                    .join("Edge")
                    .join("Application")
                    .join("msedge.exe")
            }),
    );
    for path in candidates {
        if path.is_file() {
            let install = EdgeInstall {
                version: version_next_to(&path),
                path,
            };
            if let Some(major) = install.major()
                && major < MIN_MAJOR_VERSION
            {
                return Err(BrowserError::EdgeTooOld(
                    install.version.unwrap_or_default(),
                ));
            }
            return Ok(install);
        }
    }
    Err(BrowserError::EdgeNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_highest_version_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        for d in ["98.0.1.2", "155.0.4283.45", "155.0.4283.9", "Locales"] {
            std::fs::create_dir(dir.path().join(d)).expect("mkdir");
        }
        let exe = dir.path().join("msedge.exe");
        std::fs::write(&exe, b"").expect("exe");
        assert_eq!(version_next_to(&exe).as_deref(), Some("155.0.4283.45"));
        let install = EdgeInstall {
            path: exe,
            version: Some("155.0.4283.45".into()),
        };
        assert_eq!(install.major(), Some(155));
    }
}
