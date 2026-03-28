use std::process::Command;
use tempfile::TempDir;

fn init_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .expect("git config email");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .expect("git config name");
}

fn create_commit(dir: &std::path::Path, msg: &str) {
    let file = dir.join(format!("file-{}.txt", msg.len()));
    std::fs::write(&file, msg).expect("write file");
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .output()
        .expect("git commit");
}

fn create_tag(dir: &std::path::Path, tag: &str) {
    Command::new("git")
        .args(["tag", "-a", tag, "-m", &format!("Release {tag}")])
        .current_dir(dir)
        .output()
        .expect("git tag");
}

#[test]
fn latest_tag_returns_none_when_no_tags() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");

    let tag = vership::git::latest_semver_tag(dir.path()).unwrap();
    assert!(tag.is_none());
}

#[test]
fn latest_tag_returns_most_recent() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    create_tag(dir.path(), "v0.1.0");
    create_commit(dir.path(), "second");
    create_tag(dir.path(), "v0.2.0");

    let tag = vership::git::latest_semver_tag(dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(tag, "v0.2.0");
}

#[test]
fn tag_exists_true() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    create_tag(dir.path(), "v1.0.0");

    assert!(vership::git::tag_exists(dir.path(), "v1.0.0").unwrap());
}

#[test]
fn tag_exists_false() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");

    assert!(!vership::git::tag_exists(dir.path(), "v1.0.0").unwrap());
}

#[test]
fn has_uncommitted_changes_clean() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");

    assert!(!vership::git::has_uncommitted_changes(dir.path()).unwrap());
}

#[test]
fn has_uncommitted_changes_dirty() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    // Modify a tracked file (untracked files should not block releases)
    let tracked_file = dir.path().join(format!("file-{}.txt", "initial".len()));
    std::fs::write(&tracked_file, "modified content").unwrap();

    assert!(vership::git::has_uncommitted_changes(dir.path()).unwrap());
}

#[test]
fn current_branch_is_main() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    // Set default branch name
    Command::new("git")
        .args(["checkout", "-b", "main"])
        .current_dir(dir.path())
        .output()
        .expect("checkout main");
    create_commit(dir.path(), "initial");

    let branch = vership::git::current_branch(dir.path()).unwrap();
    assert_eq!(branch, "main");
}

#[test]
fn commits_since_tag() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "feat: initial feature");
    create_tag(dir.path(), "v0.1.0");
    create_commit(dir.path(), "fix: bug fix");
    create_commit(dir.path(), "feat: new feature");

    let commits = vership::git::commits_since_tag(dir.path(), Some("v0.1.0")).unwrap();
    assert_eq!(commits.len(), 2);
    assert!(commits.iter().any(|c| c.message == "fix: bug fix"));
    assert!(commits.iter().any(|c| c.message == "feat: new feature"));
}

#[test]
fn commits_since_tag_none_gets_all() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "feat: first");
    create_commit(dir.path(), "feat: second");

    let commits = vership::git::commits_since_tag(dir.path(), None).unwrap();
    assert_eq!(commits.len(), 2);
}

#[test]
fn remote_url_from_git() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/rvben/vership.git",
        ])
        .current_dir(dir.path())
        .output()
        .expect("add remote");

    let url = vership::git::remote_url(dir.path()).unwrap().unwrap();
    assert_eq!(url, "https://github.com/rvben/vership");
}
