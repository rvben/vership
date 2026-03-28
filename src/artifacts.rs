use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::ArtifactEntry;
use crate::error::{Error, Result};
use crate::output;

/// Run all artifact commands, returning the list of files produced (relative paths).
pub fn run(root: &Path, entries: &[ArtifactEntry]) -> Result<Vec<PathBuf>> {
    let mut produced = Vec::new();

    for entry in entries {
        output::print_step(&format!("Running artifact: {}", entry.command));

        if let Some(output_path) = &entry.output {
            let cmd_output = Command::new("sh")
                .args(["-c", &entry.command])
                .current_dir(root)
                .output()
                .map_err(|e| Error::Other(format!("artifact command failed: {e}")))?;

            if !cmd_output.status.success() {
                let stderr = String::from_utf8_lossy(&cmd_output.stderr);
                return Err(Error::Other(format!(
                    "artifact '{}' failed with exit code {}: {}",
                    entry.command,
                    cmd_output.status.code().unwrap_or(-1),
                    stderr.trim()
                )));
            }

            let dest = root.join(output_path);
            std::fs::write(&dest, &cmd_output.stdout)
                .map_err(|e| Error::Other(format!("write {}: {e}", dest.display())))?;

            produced.push(PathBuf::from(output_path));
        } else {
            let status = Command::new("sh")
                .args(["-c", &entry.command])
                .current_dir(root)
                .status()
                .map_err(|e| Error::Other(format!("artifact command failed: {e}")))?;

            if !status.success() {
                return Err(Error::Other(format!(
                    "artifact '{}' failed with exit code {}",
                    entry.command,
                    status.code().unwrap_or(-1)
                )));
            }
        }

        for file in &entry.files {
            let path = root.join(file);
            if !path.exists() {
                return Err(Error::Other(format!(
                    "artifact '{}' did not produce expected file: {}",
                    entry.command, file
                )));
            }
            produced.push(PathBuf::from(file));
        }
    }

    Ok(produced)
}
