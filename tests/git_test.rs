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

fn git_command(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn untracked_files_do_not_change_the_legacy_dirty_tree_helper() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    std::fs::write(dir.path().join("forgotten.txt"), "not released").unwrap();

    assert!(!vership::git::has_tracked_changes(dir.path()).unwrap());
    assert_eq!(
        vership::git::untracked_files(dir.path()).unwrap(),
        vec!["forgotten.txt"]
    );
    assert!(!vership::git::has_uncommitted_changes(dir.path()).unwrap());
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
fn exact_ancestor_marker_is_not_hidden_by_a_newer_partial_mention() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "initial");
    let marker = "Vership-Release: v1.2.3";
    let marker_file = dir.path().join("release.txt");
    std::fs::write(&marker_file, "release").unwrap();
    git_command(dir.path(), &["add", "release.txt"]);
    git_command(
        dir.path(),
        &["commit", "-m", "chore: release", "-m", marker],
    );
    create_commit(dir.path(), "docs: explain Vership-Release: v1.2.3 marker");

    assert!(vership::git::ancestor_commit_has_marker(dir.path(), marker).unwrap());
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
fn commits_since_tag_preserves_subject_only_contract() {
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
    assert_eq!(commits[0].message, "feat: change protocol");
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
fn push_with_tag_is_atomic_when_remote_tag_conflicts() {
    let remote_dir = TempDir::new().unwrap();
    let local_dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(remote_dir.path())
        .output()
        .unwrap();
    init_git_repo(local_dir.path());
    create_commit(local_dir.path(), "initial");
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            remote_dir.path().to_str().unwrap(),
        ])
        .current_dir(local_dir.path())
        .output()
        .unwrap();
    let branch = vership::git::current_branch(local_dir.path()).unwrap();
    create_tag(local_dir.path(), "v1.0.0");
    Command::new("git")
        .args(["push", "origin", &branch, "v1.0.0"])
        .current_dir(local_dir.path())
        .output()
        .unwrap();
    let remote_before = Command::new("git")
        .args(["rev-parse", &format!("refs/heads/{branch}")])
        .current_dir(remote_dir.path())
        .output()
        .unwrap()
        .stdout;

    Command::new("git")
        .args(["tag", "--delete", "v1.0.0"])
        .current_dir(local_dir.path())
        .output()
        .unwrap();
    create_commit(local_dir.path(), "fix: candidate");
    create_tag(local_dir.path(), "v1.0.0");

    assert!(vership::git::push_with_tag(local_dir.path(), &branch, "v1.0.0").is_err());
    let remote_after = Command::new("git")
        .args(["rev-parse", &format!("refs/heads/{branch}")])
        .current_dir(remote_dir.path())
        .output()
        .unwrap()
        .stdout;
    assert_eq!(
        remote_after, remote_before,
        "branch push must roll back with tag"
    );
}

fn commit_changelog(dir: &std::path::Path, content: &str, msg: &str) -> String {
    std::fs::write(dir.join("CHANGELOG.md"), content).unwrap();
    git_command(dir, &["add", "CHANGELOG.md"]);
    git_command(dir, &["commit", "-m", msg]);
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Each commit since the tag is judged by its own diff of CHANGELOG.md: only a
/// commit that added an entry under `## [Unreleased]` counts. Editing a
/// released section, removing a note, touching other files, and anything at or
/// before the tag are all negative controls.
#[test]
fn commits_adding_unreleased_notes_judges_each_commit_by_its_own_diff() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    init_git_repo(root);
    let released = "## [0.1.0] - 2026-01-01\n\n### Added\n\n- initial\n";
    let before_tag = commit_changelog(
        root,
        &format!("# Changelog\n\n## [Unreleased]\n\n- note before the tag\n\n{released}"),
        "docs: note before the tag",
    );
    create_tag(root, "v0.1.0");

    let adds_first = commit_changelog(
        root,
        &format!(
            "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- **parser**: the fix, by hand\n\n{released}"
        ),
        "fix(parser): reject a dangling escape",
    );
    let edits_released = commit_changelog(
        root,
        &format!(
            "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- **parser**: the fix, by hand\n\n## [0.1.0] - 2026-01-01\n\n### Added\n\n- initial, reworded\n"
        ),
        "docs: reword the released note",
    );
    create_commit(root, "feat: no note of its own");
    let adds_second = commit_changelog(
        root,
        "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- **parser**: the fix, by hand\n\n### Added\n\n- **cli**: the flag, by hand\n\n## [0.1.0] - 2026-01-01\n\n### Added\n\n- initial, reworded\n",
        "feat(cli): add a flag",
    );
    let removes = commit_changelog(
        root,
        "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- **cli**: the flag, by hand\n\n## [0.1.0] - 2026-01-01\n\n### Added\n\n- initial, reworded\n",
        "docs: drop the parser note",
    );

    let noted = vership::git::commits_adding_unreleased_notes(root, Some("v0.1.0")).unwrap();
    assert_eq!(
        noted,
        vec![adds_second.clone(), adds_first.clone()],
        "newest first, only the commits that added an Unreleased entry"
    );
    for excluded in [&before_tag, &edits_released, &removes] {
        assert!(!noted.contains(excluded));
    }

    let all = vership::git::commits_adding_unreleased_notes(root, None).unwrap();
    assert_eq!(
        all,
        vec![adds_second, adds_first, before_tag],
        "without a tag the whole history is judged, including the root commit"
    );
}

#[test]
fn commits_adding_unreleased_notes_is_empty_without_a_changelog() {
    let dir = TempDir::new().unwrap();
    init_git_repo(dir.path());
    create_commit(dir.path(), "init");
    create_commit(dir.path(), "fix: something");
    assert!(
        vership::git::commits_adding_unreleased_notes(dir.path(), None)
            .unwrap()
            .is_empty()
    );
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
