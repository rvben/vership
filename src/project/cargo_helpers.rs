use std::path::Path;

use crate::error::{Error, Result};

pub fn verify_lockfile(root: &Path) -> Result<()> {
    let mut command = std::process::Command::new("cargo");
    command
        .args(["check", "--locked", "--quiet"])
        .current_dir(root);
    let status = crate::process::status_with_stdout_to_stderr(&mut command)
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
    let mut command = std::process::Command::new("cargo");
    command.args(["check", "--quiet"]).current_dir(root);
    let status = crate::process::status_with_stdout_to_stderr(&mut command)
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
    let mut fmt_command = std::process::Command::new("cargo");
    fmt_command.args(["fmt", "--", "--check"]).current_dir(root);
    let fmt_status = crate::process::status_with_stdout_to_stderr(&mut fmt_command)
        .map_err(|e| Error::Other(format!("run cargo fmt: {e}")))?;
    if !fmt_status.success() {
        return Err(Error::CheckFailed("cargo fmt check failed".to_string()));
    }

    let mut clippy_command = std::process::Command::new("cargo");
    clippy_command
        .args(["clippy", "--", "-D", "warnings"])
        .current_dir(root);
    let clippy_status = crate::process::status_with_stdout_to_stderr(&mut clippy_command)
        .map_err(|e| Error::Other(format!("run cargo clippy: {e}")))?;
    if clippy_status.success() {
        Ok(())
    } else {
        Err(Error::CheckFailed("cargo clippy failed".to_string()))
    }
}

pub fn run_tests(root: &Path) -> Result<()> {
    let mut command = std::process::Command::new("cargo");
    command.args(["test", "--quiet"]).current_dir(root);
    let status = crate::process::status_with_stdout_to_stderr(&mut command)
        .map_err(|e| Error::Other(format!("run cargo test: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CheckFailed("cargo test failed".to_string()))
    }
}
