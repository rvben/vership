use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

pub struct NodeProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl NodeProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }

    fn detect_lockfile(root: &Path) -> Option<(&'static str, &'static str)> {
        let lockfiles = [
            ("package-lock.json", "npm"),
            ("yarn.lock", "yarn"),
            ("pnpm-lock.yaml", "pnpm"),
        ];
        lockfiles
            .into_iter()
            .find(|(file, _)| root.join(file).exists())
    }

    fn has_script(root: &Path, script: &str) -> bool {
        let path = root.join("package.json");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
            return false;
        };
        parsed.get("scripts").and_then(|s| s.get(script)).is_some()
    }
}

impl Default for NodeProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for NodeProject {
    fn name(&self) -> &str {
        "Node"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let path = root.join("package.json");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read package.json: {e}")))?;
        version::parse_package_json_version(&content)
    }

    fn write_version(&self, root: &Path, new_version: &semver::Version) -> Result<()> {
        let mut modified = self.modified_files.borrow_mut();
        modified.clear();

        let path = root.join("package.json");
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read package.json: {e}")))?;
        let updated = version::replace_package_json_version(&content, new_version);
        std::fs::write(&path, updated)
            .map_err(|e| Error::Other(format!("write package.json: {e}")))?;
        modified.push(PathBuf::from("package.json"));

        if let Some((lockfile, _)) = Self::detect_lockfile(root) {
            modified.push(PathBuf::from(lockfile));
        }

        Ok(())
    }

    fn verify_lockfile(&self, root: &Path) -> Result<()> {
        if Self::detect_lockfile(root).is_none() {
            return Err(Error::CheckFailed(
                "No lockfile found. Run `npm install`, `yarn install`, or `pnpm install`."
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        let Some((_, manager)) = Self::detect_lockfile(root) else {
            return Ok(());
        };

        let (program, args): (&str, &[&str]) = match manager {
            "npm" => (
                "npm",
                &["install", "--package-lock-only", "--ignore-scripts"],
            ),
            "yarn" => ("yarn", &["install"]),
            "pnpm" => ("pnpm", &["install", "--lockfile-only"]),
            _ => return Ok(()),
        };

        let status = std::process::Command::new(program)
            .args(args)
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run {program}: {e}")))?;

        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed(format!(
                "lockfile sync failed. Run `{program} install` to fix."
            )))
        }
    }

    fn run_lint(&self, root: &Path) -> Result<()> {
        if !Self::has_script(root, "lint") {
            return Ok(());
        }

        let status = std::process::Command::new("npm")
            .args(["run", "lint"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run npm lint: {e}")))?;

        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed("lint checks failed".to_string()))
        }
    }

    fn run_tests(&self, root: &Path) -> Result<()> {
        if !Self::has_script(root, "test") {
            return Ok(());
        }

        let status = std::process::Command::new("npm")
            .args(["test"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run npm test: {e}")))?;

        if status.success() {
            Ok(())
        } else {
            Err(Error::CheckFailed("tests failed".to_string()))
        }
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        self.modified_files.borrow().clone()
    }
}
