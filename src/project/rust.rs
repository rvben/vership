use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

use super::cargo_helpers;

pub struct RustProject;

impl RustProject {
    pub fn new() -> Self {
        Self
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
        let path = root.join("Cargo.toml");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read Cargo.toml: {e}")))?;
        let updated = version::replace_cargo_toml_version(&content, new_version);
        std::fs::write(&path, updated)
            .map_err(|e| Error::Other(format!("write Cargo.toml: {e}")))?;
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
        vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")]
    }
}
