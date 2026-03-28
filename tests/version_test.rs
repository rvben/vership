use semver::Version;

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
    let updated = vership::version::replace_cargo_toml_version(content, &Version::new(1, 3, 0));
    assert!(updated.contains(r#"version = "1.3.0""#));
    assert!(updated.contains(r#"name = "example""#));
}
