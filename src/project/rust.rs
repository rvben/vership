use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

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

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("cargo")
            .args(["check", "--quiet"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "cargo check failed while syncing lockfile".to_string(),
            ))
        }
    }

    fn run_lint(&self, root: &Path) -> Result<()> {
        let fmt_status = std::process::Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo fmt: {e}")))?;
        if !fmt_status.success() {
            return Err(Error::CheckFailed(
                "cargo fmt check failed".to_string(),
            ));
        }

        let clippy_status = std::process::Command::new("cargo")
            .args(["clippy", "--", "-D", "warnings"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo clippy: {e}")))?;
        if clippy_status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "cargo clippy failed".to_string(),
            ))
        }
    }

    fn run_tests(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("cargo")
            .args(["test", "--quiet"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo test: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "cargo test failed".to_string(),
            ))
        }
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")]
    }
}
