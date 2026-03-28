use std::path::Path;

use crate::error::{Error, Result};

use super::ProjectType;
use super::rust::RustProject;
use super::rust_maturin::RustMaturinProject;

/// Detect the project type rooted at `root`.
///
/// When `project_type_override` is provided it takes precedence over auto-detection.
/// Accepted values: `"rust"`, `"rust-maturin"`.
pub fn detect(root: &Path, project_type_override: Option<&str>) -> Result<Box<dyn ProjectType>> {
    if let Some(override_type) = project_type_override {
        return match override_type {
            "rust" => Ok(Box::new(RustProject::new())),
            "rust-maturin" => Ok(Box::new(RustMaturinProject::new())),
            other => Err(Error::Config(format!(
                "unknown project type '{other}': valid values are \"rust\", \"rust-maturin\""
            ))),
        };
    }

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
