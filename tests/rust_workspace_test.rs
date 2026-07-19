use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::rust::RustProject;

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

/// Reproduces the bug: a workspace member depending on a sibling via
/// `sib = { path = "../sib", version = "X" }` must have that requirement
/// rewritten to the new shared version on bump, or the sibling becomes
/// unresolvable after a minor/major bump.
#[test]
fn workspace_bump_rewrites_sibling_dependency_version() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]
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
        "cli/Cargo.toml",
        r#"[package]
name = "acme-cli"
version.workspace = true
edition = "2021"

[dependencies]
acme-core = { path = "../core", version = "0.1.0" }
serde = "1"
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let root_content = read(dir.path(), "Cargo.toml");
    assert!(
        root_content.contains(r#"version = "0.2.0""#),
        "root workspace.package.version not bumped:\n{root_content}"
    );

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert!(
        cli_content.contains(r#"acme-core = { path = "../core", version = "0.2.0" }"#),
        "sibling dependency version not rewritten:\n{cli_content}"
    );
    assert!(
        cli_content.contains(r#"serde = "1""#),
        "external dependency was unexpectedly touched:\n{cli_content}"
    );

    let files = project.modified_files();
    assert!(
        files.contains(&PathBuf::from("Cargo.toml")),
        "modified_files missing root Cargo.toml: {files:?}"
    );
    assert!(
        files.contains(&PathBuf::from("cli").join("Cargo.toml")),
        "modified_files missing cli/Cargo.toml: {files:?}"
    );
}

/// Comments and unrelated keys on a rewritten dependency entry (features,
/// default-features) must survive the rewrite untouched: toml_edit only
/// replaces the `version` value, nothing else in the manifest.
#[test]
fn workspace_bump_preserves_comments_and_other_dep_keys() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]

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
"#,
    );
    write(
        dir.path(),
        "cli/Cargo.toml",
        r#"[package]
name = "acme-cli"
version.workspace = true

[dependencies]
# pin the core crate
acme-core = { path = "../core", version = "0.1.0", features = ["extra"], default-features = false }
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert!(
        cli_content.contains("# pin the core crate"),
        "comment was dropped:\n{cli_content}"
    );
    assert!(
        cli_content.contains(
            r#"acme-core = { path = "../core", version = "0.2.0", features = ["extra"], default-features = false }"#
        ),
        "version not rewritten in place or other keys disturbed:\n{cli_content}"
    );
}

/// A path-only dependency on a workspace member (no `version` key) has
/// nothing to rewrite and must be left byte-for-byte untouched, and must not
/// be reported as modified.
#[test]
fn workspace_bump_leaves_path_only_dependency_untouched() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]

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
"#,
    );
    let cli_manifest = r#"[package]
name = "acme-cli"
version.workspace = true

[dependencies]
acme-core = { path = "../core" }
"#;
    write(dir.path(), "cli/Cargo.toml", cli_manifest);

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert_eq!(
        cli_content, cli_manifest,
        "path-only dependency manifest was modified"
    );

    let files = project.modified_files();
    assert!(
        !files.contains(&PathBuf::from("cli").join("Cargo.toml")),
        "path-only manifest incorrectly reported as modified: {files:?}"
    );
}

/// A member dependency inheriting from `[workspace.dependencies]` via
/// `{ workspace = true }` must stay untouched (no `version` key is added
/// next to it), while the real version in the root's
/// `[workspace.dependencies]` table is rewritten.
#[test]
fn workspace_bump_leaves_workspace_true_entry_untouched_but_rewrites_workspace_dependencies() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]

[workspace.package]
version = "0.1.0"

[workspace.dependencies]
acme-core = { path = "core", version = "0.1.0" }
"#,
    );
    write(
        dir.path(),
        "core/Cargo.toml",
        r#"[package]
name = "acme-core"
version.workspace = true
"#,
    );
    write(
        dir.path(),
        "cli/Cargo.toml",
        r#"[package]
name = "acme-cli"
version.workspace = true

[dependencies]
acme-core = { workspace = true }
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let root_content = read(dir.path(), "Cargo.toml");
    assert!(
        root_content.contains(r#"acme-core = { path = "core", version = "0.2.0" }"#),
        "[workspace.dependencies] version not rewritten:\n{root_content}"
    );

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert!(
        cli_content.contains("acme-core = { workspace = true }"),
        "workspace=true entry was modified:\n{cli_content}"
    );
}

/// A renamed dependency (`foo = { package = "acme-core", ... }`) is matched
/// by its `package` field, not its key, and its version is rewritten.
#[test]
fn workspace_bump_rewrites_renamed_dependency() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]

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
"#,
    );
    write(
        dir.path(),
        "cli/Cargo.toml",
        r#"[package]
name = "acme-cli"
version.workspace = true

[dependencies]
foo = { package = "acme-core", path = "../core", version = "0.1.0" }
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert!(
        cli_content
            .contains(r#"foo = { package = "acme-core", path = "../core", version = "0.2.0" }"#),
        "renamed dependency version not rewritten:\n{cli_content}"
    );
}

/// A single-crate project (no `[workspace]` table) must keep working exactly
/// as before: no crash scanning for workspace deps, and modified_files is
/// still just the root manifest + lockfile.
#[test]
fn single_crate_write_version_has_no_workspace_deps_to_rewrite() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[package]
name = "acme-solo"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 1, 1))
        .unwrap();

    let root_content = read(dir.path(), "Cargo.toml");
    assert!(root_content.contains(r#"version = "0.1.1""#));
    assert!(root_content.contains(r#"serde = "1""#));

    let files = project.modified_files();
    assert_eq!(
        files,
        vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")]
    );
}

/// An external registry dependency that happens to share its version string
/// with the pre-bump workspace version must not be touched: it is not a
/// workspace member, matching is by name, never by coincidental version.
#[test]
fn workspace_bump_does_not_touch_external_dependency_with_matching_version_string() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"[workspace]
members = ["core", "cli"]

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
"#,
    );
    write(
        dir.path(),
        "cli/Cargo.toml",
        r#"[package]
name = "acme-cli"
version.workspace = true

[dependencies]
other-tool = "0.1.0"
"#,
    );

    let project = RustProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 2, 0))
        .unwrap();

    let cli_content = read(dir.path(), "cli/Cargo.toml");
    assert!(
        cli_content.contains(r#"other-tool = "0.1.0""#),
        "external dependency with a coincidentally matching version string was rewritten:\n{cli_content}"
    );
}
