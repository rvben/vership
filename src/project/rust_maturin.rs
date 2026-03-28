use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

pub struct RustMaturinProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl RustMaturinProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }
}

impl Default for RustMaturinProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for RustMaturinProject {
    fn name(&self) -> &str {
        "Rust + Maturin"
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

        // Update Cargo.toml
        let cargo_path = root.join("Cargo.toml");
        let content = std::fs::read_to_string(&cargo_path)
            .map_err(|e| Error::Other(format!("read Cargo.toml: {e}")))?;
        let updated = version::replace_cargo_toml_version(&content, new_version);
        std::fs::write(&cargo_path, updated)
            .map_err(|e| Error::Other(format!("write Cargo.toml: {e}")))?;
        modified.push(PathBuf::from("Cargo.toml"));
        modified.push(PathBuf::from("Cargo.lock"));

        // Update pyproject.toml version only if it has a static version field
        let pyproject_path = root.join("pyproject.toml");
        if pyproject_path.exists() {
            let content = std::fs::read_to_string(&pyproject_path)
                .map_err(|e| Error::Other(format!("read pyproject.toml: {e}")))?;
            if let Some(updated) = version::replace_pyproject_version(&content, new_version) {
                std::fs::write(&pyproject_path, updated)
                    .map_err(|e| Error::Other(format!("write pyproject.toml: {e}")))?;
                modified.push(PathBuf::from("pyproject.toml"));
            }
        }

        Ok(())
    }

    fn verify_lockfile(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("cargo")
            .args(["check", "--locked", "--quiet"])
            .current_dir(root)
            .status()
            .map_err(|e| Error::Other(format!("run cargo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "Cargo.lock is out of sync. Run `cargo check` to update it.".to_string(),
            ))
        }
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        self.modified_files.borrow().clone()
    }
}
