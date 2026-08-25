use std::path::Path;

use crate::error::{Error, Result};

use super::ProjectType;
use super::ansible::AnsibleProject;
use super::go::GoProject;
use super::gradle::GradleProject;
use super::node::NodeProject;
use super::python::PythonProject;
use super::rust::RustProject;
use super::rust_maturin::RustMaturinProject;
use crate::version;

/// Whether `root` holds an Ansible collection: a `galaxy.yml` carrying both
/// `namespace` and `name`, the two keys that form a collection's identity
/// (the FQCN `namespace.name`). Detection deliberately does not require
/// `version` — a collection with a missing or malformed version is still an
/// Ansible collection, and surfacing a precise "no version in galaxy.yml" error
/// from `read_version` is far clearer than falling through to
/// "no supported project type detected".
fn is_ansible_collection(root: &Path) -> bool {
    let galaxy = root.join("galaxy.yml");
    let Ok(content) = std::fs::read_to_string(&galaxy) else {
        return false;
    };
    ["namespace", "name"]
        .iter()
        .all(|key| version::parse_galaxy_field(&content, key).is_some())
}

/// Whether `package_json` declares itself private. A private package is never
/// published, so it is evidence of tooling vendored into the repo rather than of
/// the repo's own released identity: a Playwright harness, a docs site, a
/// workspace root. `verify` already reads a private manifest as no npm package
/// at all (`verify::targets::npm_package_name`).
///
/// A manifest that cannot be read or parsed does not count as private, so a
/// malformed `package.json` keeps today's behaviour of claiming the repo and
/// reporting a precise parse error from `read_version`.
fn is_private_package_json(package_json: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(package_json) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    parsed.get("private") == Some(&serde_json::Value::Bool(true))
}

/// Detect the project type rooted at `root`.
///
/// When `project_type_override` is provided it takes precedence over auto-detection.
/// Accepted values: `"rust"`, `"rust-maturin"`, `"node"`, `"go"`, `"python"`,
/// `"gradle"`, `"ansible-collection"` (alias `"ansible"`).
pub fn detect(root: &Path, project_type_override: Option<&str>) -> Result<Box<dyn ProjectType>> {
    if let Some(override_type) = project_type_override {
        return match override_type {
            "rust" => Ok(Box::new(RustProject::new())),
            "rust-maturin" => Ok(Box::new(RustMaturinProject::new())),
            "node" => Ok(Box::new(NodeProject::new())),
            "go" => Ok(Box::new(GoProject::new())),
            "python" => Ok(Box::new(PythonProject::new())),
            "gradle" => Ok(Box::new(GradleProject::new())),
            "ansible-collection" | "ansible" => Ok(Box::new(AnsibleProject::new())),
            other => Err(Error::Config(format!(
                "unknown project type '{other}': valid values are \"rust\", \"rust-maturin\", \"node\", \"go\", \"python\", \"gradle\", \"ansible-collection\""
            ))),
        };
    }

    // 0. galaxy.yml (namespace + name + version) → Ansible collection. Checked
    //    first so it wins over a tooling-only pyproject.toml in the same repo.
    if is_ansible_collection(root) {
        return Ok(Box::new(AnsibleProject::new()));
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

    // 3. package.json → Node, unless it is private. A private manifest is
    //    tooling, not identity, so it must not outrank the go.mod or
    //    pyproject.toml carrying the version the repo actually releases. It is
    //    still enough on its own (step 7): a private application is versioned in
    //    package.json like any other.
    if package_json.exists() && !is_private_package_json(&package_json) {
        return Ok(Box::new(NodeProject::new()));
    }

    // 4. go.mod → Go
    let go_mod = root.join("go.mod");
    if go_mod.exists() {
        return Ok(Box::new(GoProject::new()));
    }

    // 5. pyproject.toml → Python
    if pyproject_toml.exists() {
        return Ok(Box::new(PythonProject::new()));
    }

    // 6. Gradle build/settings script → Gradle
    let gradle_markers = [
        "build.gradle.kts",
        "build.gradle",
        "settings.gradle.kts",
        "settings.gradle",
    ];
    if gradle_markers.iter().any(|m| root.join(m).exists()) {
        return Ok(Box::new(GradleProject::new()));
    }

    // 7. A private package.json, now that no other marker has claimed the repo.
    //    A private Node application still carries its version here.
    if package_json.exists() {
        return Ok(Box::new(NodeProject::new()));
    }

    Err(Error::Other(
        "No supported project type detected. Supported: Rust, Rust+Maturin, Node, Go, Python, Gradle, Ansible Collection."
            .to_string(),
    ))
}
