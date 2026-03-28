use std::fs;

use tempfile::TempDir;
use vership::project;

#[test]
fn detect_rust_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_rust_maturin_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[build-system]\nrequires = [\"maturin\"]\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust + Maturin");
}

#[test]
fn detect_node_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_python_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Python");
}

#[test]
fn detect_rust_over_node() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_node_over_python() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_override_node() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("node")).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_override_python() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("python")).unwrap();
    assert_eq!(p.name(), "Python");
}

#[test]
fn detect_unknown_override() {
    let dir = TempDir::new().unwrap();
    let result = project::detect(dir.path(), Some("java"));
    assert!(result.is_err());
}

#[test]
fn detect_no_project() {
    let dir = TempDir::new().unwrap();
    let result = project::detect(dir.path(), None);
    assert!(result.is_err());
}
