use semver::Version;
use vership::version::{
    parse_cargo_toml_version, parse_package_json_version, parse_pyproject_version,
    replace_cargo_toml_version, replace_package_json_version, replace_pyproject_version,
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
