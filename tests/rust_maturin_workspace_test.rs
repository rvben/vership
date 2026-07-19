use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::rust_maturin::RustMaturinProject;

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).unwrap()
}

/// `RustMaturinProject::write_version` must rewrite intra-workspace
/// dependency versions the same way `RustProject::write_version` does: a
/// maturin-based Cargo workspace member depending on a sibling via
/// `sib = { path = "../sib", version = "X" }` must have that requirement
/// bumped, or the sibling becomes unresolvable after a minor/major bump. The
/// changed member manifest must also be reported in `modified_files()`.
#[test]
fn maturin_workspace_bump_rewrites_sibling_dependency_version() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "pyext"]
resolver = "2"

[workspace.package]
version = "0.1.0"
"#,
    );
    write(
        dir.path(),
        "core/Cargo.toml",
        r#"[package]
name = "acme-core"
version.workspace = true
edition = "2021"
"#,
    );
    write(
        dir.path(),
        "pyext/Cargo.toml",
        r#"[package]
name = "acme-pyext"
version.workspace = true
edition = "2021"

[dependencies]
acme-core = { path = "../core", version = "0.1.0" }
"#,
    );

    let project = RustMaturinProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let root_content = read(dir.path(), "Cargo.toml");
    assert!(
        root_content.contains(r#"version = "0.2.0""#),
        "root workspace.package.version not bumped:\n{root_content}"
    );

    let pyext_content = read(dir.path(), "pyext/Cargo.toml");
    assert!(
        pyext_content.contains(r#"acme-core = { path = "../core", version = "0.2.0" }"#),
        "sibling dependency version not rewritten:\n{pyext_content}"
    );

    let files = project.modified_files();
    assert!(
        files.contains(&PathBuf::from("Cargo.toml")),
        "modified_files missing root Cargo.toml: {files:?}"
    );
    assert!(
        files.contains(&PathBuf::from("pyext").join("Cargo.toml")),
        "modified_files missing pyext/Cargo.toml: {files:?}"
    );
    assert!(
        files.contains(&PathBuf::from("Cargo.lock")),
        "modified_files missing Cargo.lock: {files:?}"
    );
}
