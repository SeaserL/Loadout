// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

/// Filesystem layout under the data directory (ExecPlan 001 §5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPaths {
    pub root: PathBuf,
    pub database: PathBuf,
    pub logs: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            database: root.join("data").join("loadout.db"),
            logs: root.join("logs"),
            root,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let p = DataPaths::new(r"C:\x\Loadout");
        assert_eq!(p.database, PathBuf::from(r"C:\x\Loadout\data\loadout.db"));
        assert_eq!(p.logs, PathBuf::from(r"C:\x\Loadout\logs"));
    }
}
