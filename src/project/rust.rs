use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

use super::cargo_helpers;
use super::workspace_deps;

pub struct RustProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl RustProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }
}

impl Default for RustProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for RustProject {
    fn name(&self) -> &str {
        "Rust"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let path = root.join("Cargo.toml");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read Cargo.toml: {e}")))?;
        version::parse_cargo_toml_version(&content)
    }

    fn write_version(&self, root: &Path, new_version: &semver::Version) -> Result<()> {
        let mut modified = self.modified_files.borrow_mut();
        modified.clear();

        let path = root.join("Cargo.toml");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read Cargo.toml: {e}")))?;
        let updated = version::replace_cargo_toml_version(&content, new_version);
        std::fs::write(&path, updated)
            .map_err(|e| Error::Other(format!("write Cargo.toml: {e}")))?;
        modified.push(PathBuf::from("Cargo.toml"));

        // Rewrite `version` requirements on intra-workspace path dependencies
        // (e.g. `sib = { path = "../sib", version = "X" }`) so a sibling
        // member stays resolvable after this bump. No-op for a single-crate
        // project (no [workspace] table).
        for changed in workspace_deps::update_intra_workspace_dep_versions(root, new_version)? {
            if !modified.contains(&changed) {
                modified.push(changed);
            }
        }

        modified.push(PathBuf::from("Cargo.lock"));

        Ok(())
    }

    fn verify_lockfile(&self, root: &Path) -> Result<()> {
        cargo_helpers::verify_lockfile(root)
    }

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        cargo_helpers::sync_lockfile(root)
    }

    fn run_lint(&self, root: &Path) -> Result<()> {
        cargo_helpers::run_lint(root)
    }

    fn run_tests(&self, root: &Path) -> Result<()> {
        cargo_helpers::run_tests(root)
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        self.modified_files.borrow().clone()
    }
}
