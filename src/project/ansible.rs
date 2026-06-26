use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

const GALAXY_FILE: &str = "galaxy.yml";

/// Ansible collection projects.
///
/// The collection version lives in the `version` key of the committed
/// `galaxy.yml`. Collections are consumed by git ref, so pushing the matching
/// `v<version>` tag is the release; the version is read from and written back
/// to `galaxy.yml` with a surgical single-line edit that preserves comments,
/// key order, and quoting style.
pub struct AnsibleProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl AnsibleProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }

    fn read_galaxy(root: &Path) -> Result<String> {
        std::fs::read_to_string(root.join(GALAXY_FILE))
            .map_err(|e| Error::Other(format!("read {GALAXY_FILE}: {e}")))
    }
}

impl Default for AnsibleProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for AnsibleProject {
    fn name(&self) -> &str {
        "Ansible Collection"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let content = Self::read_galaxy(root)?;
        let raw = version::parse_galaxy_field(&content, "version")
            .ok_or_else(|| Error::Version(format!("no version in {GALAXY_FILE}")))?;
        semver::Version::parse(&raw)
            .map_err(|e| Error::Version(format!("invalid version '{raw}': {e}")))
    }

    fn write_version(&self, root: &Path, new_version: &semver::Version) -> Result<()> {
        let mut modified = self.modified_files.borrow_mut();
        modified.clear();

        let content = Self::read_galaxy(root)?;
        let updated = version::replace_galaxy_version(&content, new_version)
            .ok_or_else(|| Error::Version(format!("cannot update version in {GALAXY_FILE}")))?;

        std::fs::write(root.join(GALAXY_FILE), updated)
            .map_err(|e| Error::Other(format!("write {GALAXY_FILE}: {e}")))?;
        modified.push(PathBuf::from(GALAXY_FILE));

        Ok(())
    }

    fn verify_lockfile(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn sync_lockfile(&self, _root: &Path) -> Result<()> {
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

    fn publishes_only_git_tag(&self) -> bool {
        // Collections are installed from a git ref; the tag is the whole release.
        true
    }

    fn package_name(&self, root: &Path) -> Result<Option<String>> {
        let content = Self::read_galaxy(root)?;
        match (
            version::parse_galaxy_field(&content, "namespace"),
            version::parse_galaxy_field(&content, "name"),
        ) {
            (Some(namespace), Some(name)) => Ok(Some(format!("{namespace}.{name}"))),
            _ => Ok(None),
        }
    }
}
