use std::path::{Path, PathBuf};

use crate::error::Result;
use super::ProjectType;

pub struct RustMaturinProject;

impl RustMaturinProject {
    pub fn new() -> Self {
        Self
    }
}

impl ProjectType for RustMaturinProject {
    fn name(&self) -> &str {
        "Rust + Maturin"
    }

    fn read_version(&self, _root: &Path) -> Result<semver::Version> {
        todo!("read version from Cargo.toml")
    }

    fn write_version(&self, _root: &Path, _version: &semver::Version) -> Result<()> {
        todo!("write version to Cargo.toml + pyproject.toml")
    }

    fn verify_lockfile(&self, _root: &Path) -> Result<()> {
        todo!("cargo check --locked")
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from("Cargo.toml"),
            PathBuf::from("Cargo.lock"),
            PathBuf::from("pyproject.toml"),
        ]
    }
}
