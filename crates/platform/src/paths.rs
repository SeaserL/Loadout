// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use crate::PlatformError;

/// `%LOCALAPPDATA%\Loadout`: machine-local, per-user, not roamed (ExecPlan 001 §5.2).
pub fn default_data_dir() -> Result<PathBuf, PlatformError> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or(PlatformError::MissingEnv("LOCALAPPDATA"))?;
    Ok(PathBuf::from(base).join("Loadout"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_under_local_app_data() {
        let dir = default_data_dir().expect("data dir");
        assert!(dir.ends_with("Loadout"));
        assert!(dir.is_absolute());
    }
}
