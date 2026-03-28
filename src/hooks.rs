use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};
use crate::output;

pub fn run_hook(root: &Path, name: &str, command: Option<&str>) -> Result<()> {
    let Some(cmd) = command else {
        return Ok(());
    };
    if cmd.is_empty() {
        return Ok(());
    }

    output::print_step(&format!("Running hook: {name}"));

    let status = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(root)
        .status()
        .map_err(|e| Error::HookFailed(format!("{name}: {e}")))?;

    if !status.success() {
        return Err(Error::HookFailed(format!(
            "{name} hook failed with exit code {}",
            status.code().unwrap_or(-1)
        )));
    }

    Ok(())
}
