use std::path::Path;
use std::process::Stdio;

use crate::error::{Error, Result};

pub fn verify_lockfile(root: &Path) -> Result<()> {
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

pub fn sync_lockfile(root: &Path) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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

pub fn run_lint(root: &Path) -> Result<()> {
    let fmt_status = std::process::Command::new("cargo")
        .args(["fmt", "--", "--check"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Error::Other(format!("run cargo fmt: {e}")))?;
    if !fmt_status.success() {
        return Err(Error::CheckFailed("cargo fmt check failed".to_string()));
    }

    let clippy_status = std::process::Command::new("cargo")
        .args(["clippy", "--", "-D", "warnings"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Error::Other(format!("run cargo clippy: {e}")))?;
    if clippy_status.success() {
        Ok(())
    } else {
        Err(Error::CheckFailed("cargo clippy failed".to_string()))
    }
}

pub fn run_tests(root: &Path) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Error::Other(format!("run cargo test: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CheckFailed("cargo test failed".to_string()))
    }
}
