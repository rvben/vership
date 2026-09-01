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
fn untracked_files_make_the_working_tree_dirty() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    std::fs::write(dir.path().join("forgotten.txt"), "not released").unwrap();

    assert!(!vership::git::has_tracked_changes(dir.path()).unwrap());
    assert_eq!(
        vership::git::untracked_files(dir.path()).unwrap(),
        vec!["forgotten.txt"]
    );
    assert!(vership::git::has_uncommitted_changes(dir.path()).unwrap());
}

#[test]
fn untracked_file_names_are_nul_delimited() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    std::fs::write(dir.path().join("line\nbreak.txt"), "unusual but valid").unwrap();

    assert_eq!(
        vership::git::untracked_files(dir.path()).unwrap(),
        vec!["line\nbreak.txt"]
    );
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
fn commits_since_tag_preserves_commit_bodies() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    std::fs::write(dir.path().join("breaking.txt"), "change").unwrap();
    Command::new("git")
        .args(["add", "breaking.txt"])
        .current_dir(dir.path())
        .output()
        .expect("git add");
    Command::new("git")
        .args([
            "commit",
            "-m",
            "feat: change protocol",
            "-m",
            "BREAKING CHANGE: clients must reconnect",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git commit");

    let commits = vership::git::commits_since_tag(dir.path(), None).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject(), "feat: change protocol");
    assert!(
        commits[0]
            .message
            .contains("BREAKING CHANGE: clients must reconnect")
    );
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

#[test]
fn remote_tag_exists_checks_origin() {
    // Two repos: a bare "remote" with a pushed tag, and a local clone.
    let remote_dir = TempDir::new().unwrap();
    let local_dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(remote_dir.path())
        .output()
        .expect("git init --bare");
    init_git_repo(local_dir.path());
    create_commit(local_dir.path(), "init");
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            remote_dir.path().to_str().unwrap(),
        ])
        .current_dir(local_dir.path())
        .output()
        .expect("git remote add");
    create_tag(local_dir.path(), "v1.0.0");
    let branch = vership::git::current_branch(local_dir.path()).unwrap();
    Command::new("git")
        .args(["push", "origin", &branch, "v1.0.0"])
        .current_dir(local_dir.path())
        .output()
        .expect("git push");

    assert!(vership::git::remote_tag_exists(local_dir.path(), "v1.0.0").unwrap());
    assert!(!vership::git::remote_tag_exists(local_dir.path(), "v9.9.9").unwrap());
}

#[test]
fn has_staged_changes_reflects_index_state() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "init");

    // Clean tree: nothing staged.
    assert!(!vership::git::has_staged_changes(dir.path()).unwrap());

    // Staging an unchanged file stays a no-op.
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .expect("git add");
    assert!(!vership::git::has_staged_changes(dir.path()).unwrap());

    // A real staged change is detected.
    std::fs::write(dir.path().join("staged.txt"), "new").unwrap();
    Command::new("git")
        .args(["add", "staged.txt"])
        .current_dir(dir.path())
        .output()
        .expect("git add");
    assert!(vership::git::has_staged_changes(dir.path()).unwrap());
}
