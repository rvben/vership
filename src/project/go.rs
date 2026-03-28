use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::git;

pub struct GoProject;

impl GoProject {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GoProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for GoProject {
    fn name(&self) -> &str {
        "Go"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let tag = git::latest_semver_tag(root)?;
        match tag {
            Some(tag_str) => {
                let version_str = tag_str.strip_prefix('v').unwrap_or(&tag_str);
                semver::Version::parse(version_str)
                    .map_err(|e| Error::Version(format!("invalid version tag '{tag_str}': {e}")))
            }
            None => Ok(semver::Version::new(0, 0, 0)),
        }
    }

    fn write_version(&self, _root: &Path, _version: &semver::Version) -> Result<()> {
        Ok(())
    }

    fn verify_lockfile(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("go")
            .args(["mod", "verify"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run go mod verify: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "go mod verify failed. Run `go mod tidy` to fix.".to_string(),
            ))
        }
    }

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("go")
            .args(["mod", "tidy"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run go mod tidy: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(
                "go mod tidy failed after version bump".to_string(),
            ))
        }
    }

    fn run_lint(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("go")
            .args(["vet", "./..."])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run go vet: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed("go vet failed".to_string()))
        }
    }

    fn run_tests(&self, root: &Path) -> Result<()> {
        let status = std::process::Command::new("go")
            .args(["test", "./..."])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run go test: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed("go test failed".to_string()))
        }
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("go.mod"), PathBuf::from("go.sum")]
    }

    fn is_tag_versioned(&self) -> bool {
        true
    }
}
