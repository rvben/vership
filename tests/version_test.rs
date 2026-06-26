use semver::Version;
use vership::version::{
    parse_cargo_toml_version, parse_galaxy_field, parse_package_json_version,
    parse_pyproject_version, replace_cargo_toml_version, replace_galaxy_version,
    replace_package_json_version, replace_pyproject_version,
};

#[test]
fn bump_patch() {
    let v = Version::new(1, 2, 3);
    let bumped = vership::version::bump(v, vership::cli::BumpLevel::Patch);
    assert_eq!(bumped, Version::new(1, 2, 4));
}

#[test]
fn bump_minor() {
    let v = Version::new(1, 2, 3);
    let bumped = vership::version::bump(v, vership::cli::BumpLevel::Minor);
    assert_eq!(bumped, Version::new(1, 3, 0));
}

#[test]
fn bump_major() {
    let v = Version::new(1, 2, 3);
    let bumped = vership::version::bump(v, vership::cli::BumpLevel::Major);
    assert_eq!(bumped, Version::new(2, 0, 0));
}

#[test]
fn bump_patch_from_zero() {
    let v = Version::new(0, 0, 0);
    let bumped = vership::version::bump(v, vership::cli::BumpLevel::Patch);
    assert_eq!(bumped, Version::new(0, 0, 1));
}

#[test]
fn parse_version_from_cargo_toml() {
    let content = r#"
[package]
name = "example"
version = "1.2.3"
edition = "2024"
"#;
    let version = vership::version::parse_cargo_toml_version(content).unwrap();
    assert_eq!(version, Version::new(1, 2, 3));
}

#[test]
fn parse_version_missing() {
    let content = r#"
[package]
name = "example"
"#;
    let result = vership::version::parse_cargo_toml_version(content);
    assert!(result.is_err());
}

#[test]
fn replace_version_in_cargo_toml() {
    let content = r#"[package]
name = "example"
version = "1.2.3"
edition = "2024"
"#;
    let updated = replace_cargo_toml_version(content, &Version::new(1, 3, 0));
    assert!(updated.contains(r#"version = "1.3.0""#));
    assert!(updated.contains(r#"name = "example""#));
}

#[test]
fn parse_version_ignores_workspace_dep_versions() {
    // Cargo.toml with both a package version and a dependency with version —
    // the TOML parser must extract [package].version, not the dependency's version.
    let content = r#"[package]
name = "example"
version = "2.0.0"
edition = "2024"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
regex = "1"
"#;
    let version = parse_cargo_toml_version(content).unwrap();
    assert_eq!(version, Version::new(2, 0, 0));
}

#[test]
fn replace_version_only_updates_package_version() {
    // Ensure replacement only touches the first version = "..." in [package],
    // not dependency version strings.
    let content = r#"[package]
name = "example"
version = "1.0.0"

[dependencies]
serde = { version = "1.0" }
"#;
    let updated = replace_cargo_toml_version(content, &Version::new(1, 1, 0));
    assert!(updated.contains(r#"version = "1.1.0""#));
    // Dependency version must not be altered
    assert!(updated.contains(r#"version = "1.0""#));
}

#[test]
fn parse_version_from_pure_workspace_cargo_toml() {
    // A pure-workspace root has no [package] table; the version lives in
    // [workspace.package]. This is the common layout for multi-crate workspaces.
    let content = r#"
[workspace]
members = ["crates/foo", "crates/bar"]
resolver = "3"

[workspace.package]
version = "0.3.0"
edition = "2024"
"#;
    let version = parse_cargo_toml_version(content).unwrap();
    assert_eq!(version, Version::new(0, 3, 0));
}

#[test]
fn parse_version_falls_back_to_workspace_when_package_inherits() {
    // A crate that is also the workspace root: [package].version inherits from the
    // workspace via `version.workspace = true`, and the real version is in
    // [workspace.package].
    let content = r#"
[package]
name = "root-crate"
version.workspace = true

[workspace]
members = ["."]

[workspace.package]
version = "1.4.2"
"#;
    let version = parse_cargo_toml_version(content).unwrap();
    assert_eq!(version, Version::new(1, 4, 2));
}

#[test]
fn replace_pyproject_version_with_static_version() {
    let content = r#"[project]
name = "example"
version = "1.0.0"
"#;
    let result = replace_pyproject_version(content, &Version::new(1, 1, 0));
    assert!(result.is_some());
    assert!(result.unwrap().contains(r#"version = "1.1.0""#));
}

#[test]
fn replace_pyproject_version_skips_dynamic_version() {
    let content = r#"[project]
name = "example"
dynamic = ["version"]
"#;
    let result = replace_pyproject_version(content, &Version::new(1, 1, 0));
    assert!(result.is_none());
}

#[test]
fn replace_pyproject_version_with_non_version_dynamic() {
    // dynamic list exists but does not include "version" — should still replace
    let content = r#"[project]
name = "example"
version = "0.5.0"
dynamic = ["description"]
"#;
    let result = replace_pyproject_version(content, &Version::new(0, 6, 0));
    assert!(result.is_some());
    assert!(result.unwrap().contains(r#"version = "0.6.0""#));
}

#[test]
fn replace_pyproject_version_returns_none_when_no_version_field() {
    let content = r#"[project]
name = "example"
dynamic = ["description"]
"#;
    let result = replace_pyproject_version(content, &Version::new(1, 0, 0));
    assert!(result.is_none());
}

#[test]
fn parse_version_from_package_json() {
    let content = r#"{
  "name": "my-app",
  "version": "2.1.0",
  "description": "test"
}"#;
    let version = parse_package_json_version(content).unwrap();
    assert_eq!(version, Version::new(2, 1, 0));
}

#[test]
fn parse_version_missing_from_package_json() {
    let content = r#"{
  "name": "my-app",
  "description": "test"
}"#;
    let result = parse_package_json_version(content);
    assert!(result.is_err());
}

#[test]
fn replace_version_in_package_json() {
    let content = r#"{
  "name": "my-app",
  "version": "1.0.0",
  "dependencies": {
    "lodash": "^4.17.0"
  }
}"#;
    let updated = replace_package_json_version(content, &Version::new(1, 1, 0));
    assert!(updated.contains(r#""version": "1.1.0""#));
    assert!(updated.contains(r#""lodash": "^4.17.0""#));
}

#[test]
fn parse_version_from_pyproject_toml() {
    let content = r#"[project]
name = "my-app"
version = "3.2.1"
"#;
    let version = parse_pyproject_version(content).unwrap();
    assert_eq!(version, Version::new(3, 2, 1));
}

#[test]
fn parse_version_from_pyproject_toml_dynamic() {
    let content = r#"[project]
name = "my-app"
dynamic = ["version"]
"#;
    let result = parse_pyproject_version(content);
    assert!(result.is_err());
}

#[test]
fn parse_version_from_pyproject_toml_no_project_section() {
    let content = r#"[tool.setuptools]
packages = ["myapp"]
"#;
    let result = parse_pyproject_version(content);
    assert!(result.is_err());
}

#[test]
fn parse_galaxy_quoted_version() {
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\"\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("0.0.2")
    );
}

#[test]
fn parse_galaxy_unquoted_version() {
    let content = "namespace: hda\nname: platform\nversion: 0.0.2\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("0.0.2")
    );
}

#[test]
fn parse_galaxy_single_quoted_version() {
    let content = "version: '1.4.0'\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("1.4.0")
    );
}

#[test]
fn parse_galaxy_version_with_trailing_comment() {
    let content = "version: \"0.0.2\"  # bump me\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("0.0.2")
    );
}

#[test]
fn parse_galaxy_namespace_and_name_fields() {
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\"\n";
    assert_eq!(
        parse_galaxy_field(content, "namespace").as_deref(),
        Some("hda")
    );
    assert_eq!(
        parse_galaxy_field(content, "name").as_deref(),
        Some("platform")
    );
}

#[test]
fn parse_galaxy_does_not_match_nested_version_key() {
    // A `version:` nested under another mapping (indented) must not be read as
    // the collection version; only the column-0 top-level key counts.
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\"\ndependencies:\n  some.dep:\n    version: \"9.9.9\"\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("0.0.2")
    );
}

#[test]
fn parse_galaxy_version_absent() {
    let content = "namespace: hda\nname: platform\n";
    assert_eq!(parse_galaxy_field(content, "version"), None);
}

#[test]
fn replace_galaxy_version_preserves_double_quotes() {
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\"\n";
    let updated = replace_galaxy_version(content, &Version::new(0, 0, 3)).unwrap();
    assert!(updated.contains("version: \"0.0.3\""));
    assert!(!updated.contains("0.0.2"));
}

#[test]
fn replace_galaxy_version_keeps_unquoted_unquoted() {
    let content = "version: 0.0.2\n";
    let updated = replace_galaxy_version(content, &Version::new(0, 0, 3)).unwrap();
    assert_eq!(updated, "version: 0.0.3\n");
}

#[test]
fn replace_galaxy_version_preserves_single_quotes() {
    let content = "version: '0.0.2'\n";
    let updated = replace_galaxy_version(content, &Version::new(0, 0, 3)).unwrap();
    assert_eq!(updated, "version: '0.0.3'\n");
}

#[test]
fn replace_galaxy_version_preserves_comments_and_key_order() {
    // Leading comments + non-alphabetical key order: only the version line changes.
    let content = "# managed by vership\nname: platform\nnamespace: hda\nreadme: README.md\nversion: \"0.0.2\"  # current\nauthors:\n  - Someone\n";
    let updated = replace_galaxy_version(content, &Version::new(0, 1, 0)).unwrap();
    let expected = "# managed by vership\nname: platform\nnamespace: hda\nreadme: README.md\nversion: \"0.1.0\"  # current\nauthors:\n  - Someone\n";
    assert_eq!(updated, expected);
}

#[test]
fn replace_galaxy_version_returns_none_when_absent() {
    let content = "namespace: hda\nname: platform\n";
    assert!(replace_galaxy_version(content, &Version::new(0, 0, 3)).is_none());
}

#[test]
fn parse_galaxy_version_rejects_unterminated_quote() {
    // Opening quote with no closing quote is a malformed manifest; do not
    // silently accept it as a valid version.
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\n";
    assert_eq!(parse_galaxy_field(content, "version"), None);
}

#[test]
fn parse_galaxy_version_rejects_mismatched_quotes() {
    let content = "version: \"0.0.2'\n";
    assert_eq!(parse_galaxy_field(content, "version"), None);
}

#[test]
fn replace_galaxy_version_returns_none_on_unterminated_quote() {
    let content = "namespace: hda\nname: platform\nversion: \"0.0.2\n";
    assert!(replace_galaxy_version(content, &Version::new(0, 0, 3)).is_none());
}

#[test]
fn parse_galaxy_version_with_crlf_line_endings() {
    let content = "namespace: hda\r\nname: platform\r\nversion: \"0.0.2\"\r\n";
    assert_eq!(
        parse_galaxy_field(content, "version").as_deref(),
        Some("0.0.2")
    );
}

#[test]
fn replace_galaxy_version_preserves_crlf_line_endings() {
    let content = "namespace: hda\r\nname: platform\r\nversion: \"0.0.2\"\r\n";
    let updated = replace_galaxy_version(content, &Version::new(0, 0, 3)).unwrap();
    assert_eq!(
        updated,
        "namespace: hda\r\nname: platform\r\nversion: \"0.0.3\"\r\n"
    );
}
