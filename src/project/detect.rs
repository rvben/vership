use std::path::Path;

use crate::error::{Error, Result};

use super::ProjectType;
use super::rust::RustProject;
use super::rust_maturin::RustMaturinProject;

pub fn detect(root: &Path) -> Result<Box<dyn ProjectType>> {
    let cargo_toml = root.join("Cargo.toml");
    let pyproject_toml = root.join("pyproject.toml");

    if cargo_toml.exists() && pyproject_toml.exists() {
        let content = std::fs::read_to_string(&pyproject_toml)
            .map_err(|e| Error::Other(format!("read pyproject.toml: {e}")))?;
        if content.contains("maturin") {
            return Ok(Box::new(RustMaturinProject::new()));
        }
    }

    if cargo_toml.exists() {
        return Ok(Box::new(RustProject::new()));
    }

    Err(Error::Other(
        "No supported project type detected. vership currently supports Rust and Rust+Maturin projects.".to_string(),
    ))
}
