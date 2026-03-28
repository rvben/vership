use std::fs;

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::node::NodeProject;

#[test]
fn node_read_version() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "test-app",
  "version": "1.2.3"
}"#,
    )
    .unwrap();

    let project = NodeProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(1, 2, 3));
}

#[test]
fn node_write_version() {
    let dir = TempDir::new().unwrap();
    let pkg = dir.path().join("package.json");
    fs::write(
        &pkg,
        r#"{
  "name": "test-app",
  "version": "1.0.0",
  "dependencies": {
    "lodash": "^4.17.0"
  }
}"#,
    )
    .unwrap();

    let project = NodeProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 1, 0))
        .unwrap();

    let content = fs::read_to_string(&pkg).unwrap();
    assert!(content.contains(r#""version": "1.1.0""#));
    assert!(content.contains(r#""lodash": "^4.17.0""#));
}

#[test]
fn node_modified_files_with_npm() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package-lock.json"), "{}").unwrap();

    let project = NodeProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 1))
        .unwrap();

    let files = project.modified_files();
    assert!(files.iter().any(|f| f.to_str() == Some("package.json")));
    assert!(files
        .iter()
        .any(|f| f.to_str() == Some("package-lock.json")));
}

#[test]
fn node_modified_files_without_lockfile() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();

    let project = NodeProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 1))
        .unwrap();

    let files = project.modified_files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].to_str(), Some("package.json"));
}

#[test]
fn node_name() {
    let project = NodeProject::new();
    assert_eq!(project.name(), "Node");
}
