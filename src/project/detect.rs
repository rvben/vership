use std::path::Path;

use crate::error::{Error, Result};

use super::ProjectType;
use super::node::NodeProject;
use super::python::PythonProject;
use super::rust::RustProject;
use super::rust_maturin::RustMaturinProject;

/// Detect the project type rooted at `root`.
///
/// When `project_type_override` is provided it takes precedence over auto-detection.
/// Accepted values: `"rust"`, `"rust-maturin"`, `"node"`, `"python"`.
pub fn detect(root: &Path, project_type_override: Option<&str>) -> Result<Box<dyn ProjectType>> {
    if let Some(override_type) = project_type_override {
        return match override_type {
            "rust" => Ok(Box::new(RustProject::new())),
            "rust-maturin" => Ok(Box::new(RustMaturinProject::new())),
            "node" => Ok(Box::new(NodeProject::new())),
            "python" => Ok(Box::new(PythonProject::new())),
            other => Err(Error::Config(format!(
                "unknown project type '{other}': valid values are \"rust\", \"rust-maturin\", \"node\", \"python\""
            ))),
        };
    }

    let cargo_toml = root.join("Cargo.toml");
    let pyproject_toml = root.join("pyproject.toml");
    let package_json = root.join("package.json");

    // 1. Cargo.toml + pyproject.toml with maturin → RustMaturin
    if cargo_toml.exists() && pyproject_toml.exists() {
        let content = std::fs::read_to_string(&pyproject_toml)
            .map_err(|e| Error::Other(format!("read pyproject.toml: {e}")))?;
        if content.contains("maturin") {
            return Ok(Box::new(RustMaturinProject::new()));
        }
    }

    // 2. Cargo.toml → Rust
    if cargo_toml.exists() {
        return Ok(Box::new(RustProject::new()));
    }

    // 3. package.json → Node
    if package_json.exists() {
        return Ok(Box::new(NodeProject::new()));
    }

    // 4. pyproject.toml → Python
    if pyproject_toml.exists() {
        return Ok(Box::new(PythonProject::new()));
    }

    Err(Error::Other(
        "No supported project type detected. Supported: Rust, Rust+Maturin, Node, Python."
            .to_string(),
    ))
}
