use std::fs;

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::python::PythonProject;

#[test]
fn python_read_version() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"[project]
name = "my-app"
version = "0.5.0"
"#,
    )
    .unwrap();

    let project = PythonProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 5, 0));
}

#[test]
fn python_read_version_dynamic_fails() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"[project]
name = "my-app"
dynamic = ["version"]
"#,
    )
    .unwrap();

    let project = PythonProject::new();
    let result = project.read_version(dir.path());
    assert!(result.is_err());
}

#[test]
fn python_write_version() {
    let dir = TempDir::new().unwrap();
    let pyproject = dir.path().join("pyproject.toml");
    fs::write(
        &pyproject,
        r#"[project]
name = "my-app"
version = "1.0.0"
description = "test"
"#,
    )
    .unwrap();

    let project = PythonProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 1, 0))
        .unwrap();

    let content = fs::read_to_string(&pyproject).unwrap();
    assert!(content.contains(r#"version = "1.1.0""#));
    assert!(content.contains(r#"name = "my-app""#));
}

#[test]
fn python_write_version_dynamic_fails() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"[project]
name = "my-app"
dynamic = ["version"]
"#,
    )
    .unwrap();

    let project = PythonProject::new();
    let result = project.write_version(dir.path(), &Version::new(1, 0, 0));
    assert!(result.is_err());
}

#[test]
fn python_modified_files_with_uv_lock() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"[project]
name = "my-app"
version = "1.0.0"
"#,
    )
    .unwrap();
    fs::write(dir.path().join("uv.lock"), "").unwrap();

    let project = PythonProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 1))
        .unwrap();

    let files = project.modified_files();
    assert!(files.iter().any(|f| f.to_str() == Some("pyproject.toml")));
    assert!(files.iter().any(|f| f.to_str() == Some("uv.lock")));
}

#[test]
fn python_name() {
    let project = PythonProject::new();
    assert_eq!(project.name(), "Python");
}
