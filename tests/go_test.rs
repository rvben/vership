use std::fs;
use std::path::PathBuf;
use std::process::Command;

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::go::GoProject;

fn init_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    fs::write(dir.join("go.mod"), "module example.com/test\n\ngo 1.21\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn go_name() {
    let project = GoProject::new();
    assert_eq!(project.name(), "Go");
}

#[test]
fn go_is_tag_versioned() {
    let project = GoProject::new();
    assert!(project.is_tag_versioned());
}

#[test]
fn go_read_version_from_tag() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());

    Command::new("git")
        .args(["tag", "-a", "v1.2.3", "-m", "Release v1.2.3"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let project = GoProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(1, 2, 3));
}

#[test]
fn go_read_version_no_tags_returns_zero() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());

    let project = GoProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 0, 0));
}

#[test]
fn go_read_version_latest_tag() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());

    Command::new("git")
        .args(["tag", "-a", "v0.1.0", "-m", "Release v0.1.0"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    fs::write(dir.path().join("main.go"), "package main\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["tag", "-a", "v0.2.0", "-m", "Release v0.2.0"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let project = GoProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 2, 0));
}

#[test]
fn go_write_version_is_noop() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());

    let content_before = fs::read_to_string(dir.path().join("go.mod")).unwrap();

    let project = GoProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 0))
        .unwrap();

    let content_after = fs::read_to_string(dir.path().join("go.mod")).unwrap();
    assert_eq!(content_before, content_after);
}

#[test]
fn go_modified_files() {
    let project = GoProject::new();
    let files = project.modified_files();
    assert_eq!(
        files,
        vec![PathBuf::from("go.mod"), PathBuf::from("go.sum")]
    );
}
