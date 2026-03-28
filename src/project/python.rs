use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

pub struct PythonProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl PythonProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }

    fn detect_lockfile(root: &Path) -> Option<&'static str> {
        let lockfiles = ["uv.lock", "poetry.lock"];
        lockfiles.into_iter().find(|f| root.join(f).exists())
    }
}

impl Default for PythonProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for PythonProject {
    fn name(&self) -> &str {
        "Python"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let path = root.join("pyproject.toml");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read pyproject.toml: {e}")))?;
        version::parse_pyproject_version(&content)
    }

    fn write_version(&self, root: &Path, new_version: &semver::Version) -> Result<()> {
        let mut modified = self.modified_files.borrow_mut();
        modified.clear();

        let path = root.join("pyproject.toml");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read pyproject.toml: {e}")))?;

        let updated =
            version::replace_pyproject_version(&content, new_version).ok_or_else(|| {
                Error::Version(
                    "cannot update version in pyproject.toml: version is dynamic or missing"
                        .to_string(),
                )
            })?;

        std::fs::write(&path, updated)
            .map_err(|e| Error::Other(format!("write pyproject.toml: {e}")))?;
        modified.push(PathBuf::from("pyproject.toml"));

        if let Some(lockfile) = Self::detect_lockfile(root) {
            modified.push(PathBuf::from(lockfile));
        }

        Ok(())
    }

    fn verify_lockfile(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        if root.join("uv.lock").exists() {
            let status = std::process::Command::new("uv")
                .args(["lock"])
                .current_dir(root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| Error::Other(format!("run uv lock: {e}")))?;
            if !status.success() {
                return Err(Error::CheckFailed(
                    "uv lock failed after version bump".to_string(),
                ));
            }
        } else if root.join("poetry.lock").exists() {
            let status = std::process::Command::new("poetry")
                .args(["lock", "--no-update"])
                .current_dir(root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| Error::Other(format!("run poetry lock: {e}")))?;
            if !status.success() {
                return Err(Error::CheckFailed(
                    "poetry lock failed after version bump".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn run_lint(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn run_tests(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        self.modified_files.borrow().clone()
    }
}
