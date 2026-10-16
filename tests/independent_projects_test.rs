//! Real CLI contracts: project selection, independent releases and safe recovery.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vership"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn succeeds(root: &Path, args: &[&str]) -> String {
    let out = cli(root, args);
    assert!(
        out.status.success(),
        "vership {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn fails(root: &Path, args: &[&str], message: &str) {
    let out = cli(root, args);
    assert!(!out.status.success(), "unexpected success: {args:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(message),
        "expected {message}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn commit(root: &Path, path: &str, text: &str, message: &str) {
    fs::write(root.join(path), text).unwrap();
    git(root, &["add", "--", path]);
    git(root, &["commit", "-m", message]);
}
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "tag.gpgsign", "false"]);
    fs::create_dir(root.join("agent")).unwrap();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0.0'\n",
    )
    .unwrap();
    fs::write(root.join("vership.toml"), "[project]\ntype = 'python'\n[checks]\nlint = false\ntests = false\n[projects.agent]\npath = 'agent'\ntag_prefix = 'agent-v'\npaths = ['agent', 'shared']\n").unwrap();
    fs::write(
        root.join("agent/pyproject.toml"),
        "[project]\nname = 'app-agent'\nversion = '0.4.1'\n",
    )
    .unwrap();
    fs::write(root.join("agent/vership.toml"), "[project]\ntype = 'python'\n[checks]\nlint = false\ntests = false\n[verify]\ntargets = ['tag', 'pypi']\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "v1.0.0"]);
    git(root, &["tag", "agent-v0.4.1"]);
    dir
}
fn status(root: &Path, selected: bool) -> serde_json::Value {
    let args = if selected {
        vec!["--project", "agent", "status"]
    } else {
        vec!["status"]
    };
    serde_json::from_str(&succeeds(root, &args)).unwrap()
}

#[test]
fn sequential_releases_keep_versions_files_and_tags_independent() {
    let dir = fixture();
    let root = dir.path();
    commit(root, "agent/code.py", "agent", "feat: agent feature");
    let original_root = fs::read(root.join("pyproject.toml")).unwrap();
    succeeds(root, &["--project", "agent", "bump", "minor", "--prepare"]);
    assert_eq!(
        fs::read(root.join("pyproject.toml")).unwrap(),
        original_root
    );
    assert!(!root.join("CHANGELOG.md").exists());
    assert!(!git(root, &["tag"]).contains("agent-v0.5.0"));
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
    assert_eq!(status(root, true)["latest_tag"], "agent-v0.5.0");
    assert_eq!(status(root, false)["latest_tag"], "v1.0.0");
    let agent_manifest = fs::read(root.join("agent/pyproject.toml")).unwrap();
    commit(root, "app.py", "root", "fix: root bug");
    succeeds(root, &["bump", "patch", "--prepare"]);
    succeeds(root, &["release", "--no-push"]);
    assert_eq!(
        fs::read(root.join("agent/pyproject.toml")).unwrap(),
        agent_manifest
    );
    let changelog = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("root bug"));
    assert!(!changelog.contains("agent feature"));
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    assert_eq!(status(root, false)["latest_tag"], "v1.0.1");
}

#[test]
fn history_uses_paths_including_shared_and_mixed_commits() {
    let dir = fixture();
    let root = dir.path();
    commit(root, "agent/code.py", "a", "feat: agent only");
    commit(root, "app.py", "a", "fix: root only");
    commit(root, "shared", "a", "feat: shared behavior");
    fs::write(root.join("app.py"), "b").unwrap();
    fs::write(root.join("agent/code.py"), "b").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fix: mixed behavior"]);
    let root_status = status(root, false).to_string();
    let child_status = status(root, true).to_string();
    for text in [&root_status, &child_status] {
        assert!(text.contains("shared behavior"));
        assert!(text.contains("mixed behavior"));
    }
    assert!(!root_status.contains("agent only"));
    assert!(!child_status.contains("root only"));
}

#[test]
fn explicit_selection_is_required_even_when_called_inside_child() {
    let dir = fixture();
    let root = dir.path();
    fails(&root.join("agent"), &["status"], "select it explicitly");
    assert_eq!(
        status(&root.join("agent"), true)["current_version"],
        "0.4.1"
    );
    fails(root, &["--project", "missing", "status"], "unknown project");
}

#[test]
fn dry_run_leaves_worktree_index_tags_and_head_untouched() {
    let dir = fixture();
    let root = dir.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let tags = git(root, &["tag"]);
    succeeds(root, &["--project", "agent", "bump", "patch", "--dry-run"]);
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(root, &["tag"]), tags);
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn prepared_release_refuses_new_head_and_refreshes_without_replaying_hooks() {
    let dir = fixture();
    let marker = TempDir::new().unwrap();
    let root = dir.path();
    let hook = format!(
        "[hooks]\npre-bump = \"printf x >> '{}'\"\n",
        marker.path().join("runs").display()
    );
    let config = fs::read_to_string(root.join("agent/vership.toml")).unwrap();
    commit(
        root,
        "agent/vership.toml",
        &(config + &hook),
        "chore: configure hook",
    );
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    commit(root, "app.py", "other", "fix: other release");
    fails(
        root,
        &["--project", "agent", "release", "--no-push"],
        "HEAD advanced",
    );
    succeeds(root, &["--project", "agent", "release", "--prepare"]);
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
    assert_eq!(fs::read_to_string(marker.path().join("runs")).unwrap(), "x");
    assert_eq!(
        git(root, &["rev-parse", "HEAD"]),
        git(root, &["rev-parse", "agent-v0.4.2^{}"])
    );
}

#[test]
fn resume_preserves_foreign_staged_and_unstaged_files() {
    for staged in [false, true] {
        let dir = fixture();
        let root = dir.path();
        commit(root, "app.py", "old", "chore: add root file");
        fs::write(
            root.join("agent/pyproject.toml"),
            "[project]\nname='app-agent'\nversion='0.4.2'\n",
        )
        .unwrap();
        fs::write(root.join("app.py"), "unrelated").unwrap();
        if staged {
            git(root, &["add", "app.py"]);
        }
        let before = git(root, &["status", "--porcelain"]);
        fails(
            root,
            &["--project", "agent", "resume", "--no-push"],
            "unexpected release change",
        );
        assert_eq!(git(root, &["status", "--porcelain"]), before);
        assert_eq!(
            fs::read_to_string(root.join("app.py")).unwrap(),
            "unrelated"
        );
    }
}

#[test]
fn resume_accepts_only_selected_manifest_and_declared_version_files() {
    let dir = fixture();
    let root = dir.path();
    fs::write(
        root.join("agent/pyproject.toml"),
        "[project]\nname='app-agent'\nversion='0.4.2'\n",
    )
    .unwrap();
    succeeds(root, &["--project", "agent", "resume", "--no-push"]);
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    assert!(git(root, &["show", "agent-v0.4.2:agent/pyproject.toml"]).contains("0.4.2"));
}

#[test]
fn hook_cannot_sweep_root_changes_into_agent_commit() {
    let dir = fixture();
    let root = dir.path();
    let config = fs::read_to_string(root.join("agent/vership.toml")).unwrap();
    commit(
        root,
        "agent/vership.toml",
        &(config + "\n[hooks]\npost-bump = 'echo unexpected > ../foreign; git add ../foreign'\n"),
        "chore: hook",
    );
    let head = git(root, &["rev-parse", "HEAD"]);
    fails(
        root,
        &["--project", "agent", "bump", "patch", "--prepare"],
        "unexpected release change",
    );
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert!(root.join("foreign").exists());
}

#[test]
fn semantic_tags_ignore_invalid_tags_and_sort_prereleases_correctly() {
    let dir = fixture();
    let root = dir.path();
    for tag in [
        "agent-vgarbage",
        "agent-v0.9.0-rc.10",
        "agent-v0.9.0-rc.2",
        "v90.0.0",
    ] {
        git(root, &["tag", tag]);
    }
    assert_eq!(status(root, true)["latest_tag"], "agent-v0.9.0-rc.10");
}

#[test]
fn unmerged_higher_tag_blocks_release_in_selected_namespace() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["checkout", "-b", "other"]);
    commit(root, "agent/branch", "other", "feat: unmerged");
    git(root, &["tag", "agent-v0.8.0"]);
    git(root, &["checkout", "main"]);
    fails(
        root,
        &["--project", "agent", "bump", "patch", "--prepare"],
        "not reachable",
    );
    assert_eq!(status(root, true)["latest_tag"], "agent-v0.4.1");
}

#[test]
fn linked_worktree_resolves_repository_from_git_not_dot_git_directory() {
    let dir = fixture();
    let worktree = TempDir::new().unwrap();
    let root = dir.path();
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "test-worktree",
            worktree.path().to_str().unwrap(),
        ],
    );
    assert_eq!(status(worktree.path(), true)["current_version"], "0.4.1");
    fails(
        worktree.path(),
        &["--project", "agent", "bump", "patch", "--prepare"],
        "switch to 'main'",
    );
}

#[test]
fn invalid_registries_fail_before_mutation() {
    for registry in [
        "[projects.other]\npath='agent'\ntag_prefix='other-v'\n",
        "[projects.other]\npath='../escape'\ntag_prefix='other-v'\n",
        "[projects.other]\npath='agent/nested'\ntag_prefix='other-v'\n",
    ] {
        let dir = fixture();
        let root = dir.path();
        fs::create_dir(root.join("agent/nested")).unwrap();
        let mut config = fs::read_to_string(root.join("vership.toml")).unwrap();
        config.push_str(registry);
        fs::write(root.join("vership.toml"), config).unwrap();
        let out = cli(root, &["--project", "agent", "status"]);
        assert!(!out.status.success());
    }
}

#[cfg(unix)]
#[test]
fn symlinked_manifest_cannot_modify_other_project() {
    let dir = fixture();
    let root = dir.path();
    fs::remove_file(root.join("agent/pyproject.toml")).unwrap();
    std::os::unix::fs::symlink("../pyproject.toml", root.join("agent/pyproject.toml")).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: symlink"]);
    let before = fs::read(root.join("pyproject.toml")).unwrap();
    fails(
        root,
        &["--project", "agent", "bump", "patch", "--prepare"],
        "escapes project",
    );
    assert_eq!(fs::read(root.join("pyproject.toml")).unwrap(), before);
}

#[test]
fn selected_go_project_finishes_preparation_without_root_tags_interfering() {
    let dir = fixture();
    let root = dir.path();
    fs::remove_file(root.join("agent/pyproject.toml")).unwrap();
    fs::write(
        root.join("agent/go.mod"),
        "module example.invalid/agent\n\ngo 1.23\n",
    )
    .unwrap();
    fs::write(
        root.join("agent/vership.toml"),
        "[project]\ntype='go'\n[checks]\nlint=false\ntests=false\n[verify]\ntargets=['tag']\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: Go package"]);
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    assert_eq!(status(root, true)["current_version"], "0.4.2");
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
    assert_eq!(status(root, true)["latest_tag"], "agent-v0.4.2");
}

#[test]
fn selected_changelog_links_and_curated_coverage_use_exact_tag_and_path() {
    let dir = fixture();
    let root = dir.path();
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/fixture.git",
        ],
    );
    // Read-only preview doesn't consult remote tags when the on-disk tag isn't marked.
    commit(
        root,
        "agent/CHANGELOG.md",
        "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- Custom note\n",
        "fix: already documented change",
    );
    let preview = succeeds(root, &["--project", "agent", "changelog"]);
    assert!(
        preview.contains("compare/agent-v0.4.1...agent-v0.4.2"),
        "{preview}"
    );
    assert!(preview.contains("Custom note"));
    assert!(!preview.contains("already documented change"));
}

#[test]
fn two_releases_push_exact_tags_and_commits_to_local_remote() {
    let dir = fixture();
    let bare = TempDir::new().unwrap();
    let root = dir.path();
    git(bare.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "--tags"]);
    commit(root, "agent/code.py", "release", "feat: publish agent");
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    succeeds(root, &["--project", "agent", "release"]);
    let agent_head = git(root, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(bare.path(), &["rev-parse", "agent-v0.4.2^{}"]),
        agent_head
    );
    commit(root, "app.py", "release", "fix: publish app");
    succeeds(root, &["bump", "patch", "--prepare"]);
    succeeds(root, &["release"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    assert_eq!(git(bare.path(), &["rev-parse", "main"]), head);
    assert_eq!(git(bare.path(), &["rev-parse", "v1.0.1^{}"]), head);
    assert_eq!(
        git(bare.path(), &["rev-parse", "agent-v0.4.2^{}"]),
        agent_head
    );
    succeeds(root, &["--project", "agent", "verify", "--targets", "tag"]);
}

#[test]
fn failed_push_retry_does_not_replay_preparation_hooks() {
    let dir = fixture();
    let marker = TempDir::new().unwrap();
    let bare = TempDir::new().unwrap();
    let root = dir.path();
    git(bare.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "--tags"]);
    let config = fs::read_to_string(root.join("agent/vership.toml")).unwrap();
    let hook = format!(
        "\n[hooks]\npre-bump = \"printf x >> '{}'\"\npre-push = \"test -f '{}'\"\n",
        marker.path().join("runs").display(),
        marker.path().join("allow").display()
    );
    commit(
        root,
        "agent/vership.toml",
        &(config + &hook),
        "chore: retry hooks",
    );
    let out = cli(root, &["--project", "agent", "bump", "patch"]);
    assert!(!out.status.success());
    assert!(git(root, &["tag"]).contains("agent-v0.4.2"));
    fs::write(marker.path().join("allow"), "").unwrap();
    succeeds(root, &["--project", "agent", "release"]);
    assert_eq!(fs::read_to_string(marker.path().join("runs")).unwrap(), "x");
    assert_eq!(
        git(bare.path(), &["rev-parse", "agent-v0.4.2^{}"]),
        git(root, &["rev-parse", "HEAD"])
    );
}

#[test]
fn branch_is_inherited_and_an_explicit_conflict_is_rejected() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["branch", "-m", "trunk"]);
    let config = fs::read_to_string(root.join("vership.toml"))
        .unwrap()
        .replace("type = 'python'", "type = 'python'\nbranch = 'trunk'");
    commit(root, "vership.toml", &config, "chore: trunk");
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    let config = fs::read_to_string(root.join("agent/vership.toml"))
        .unwrap()
        .replace("type = 'python'", "type = 'python'\nbranch = 'main'");
    fs::write(root.join("agent/vership.toml"), config).unwrap();
    fails(root, &["--project", "agent", "status"], "branch must match");
}

#[test]
fn ambiguous_namespaces_are_rejected_before_file_mutation() {
    for prefix in ["v", "v1.0.0-", "bad*", "bad..tag", "bad#tag"] {
        let dir = fixture();
        let root = dir.path();
        let config = fs::read_to_string(root.join("vership.toml"))
            .unwrap()
            .replace("agent-v", prefix);
        fs::write(root.join("vership.toml"), config).unwrap();
        fails(
            root,
            &["--project", "agent", "bump", "patch", "--prepare"],
            "Configuration error",
        );
        assert!(
            fs::read_to_string(root.join("agent/pyproject.toml"))
                .unwrap()
                .contains("0.4.1")
        );
    }
}

#[test]
fn root_version_glob_cannot_claim_a_registered_packages_manifest() {
    let dir = fixture();
    let root = dir.path();
    let config = fs::read_to_string(root.join("vership.toml")).unwrap()
        + "\n[[version_files]]\nglob='agent/pyproject.toml'\nsearch='0.4.1'\nreplace='{version}'\n";
    commit(
        root,
        "vership.toml",
        &config,
        "chore: invalid version scope",
    );
    fails(
        root,
        &["bump", "patch", "--prepare"],
        "another registered project",
    );
    assert!(
        fs::read_to_string(root.join("agent/pyproject.toml"))
            .unwrap()
            .contains("0.4.1")
    );
}

#[test]
fn named_project_refuses_implicit_parent_workspace_lock_ownership() {
    for (manifest, body) in [
        (
            "pyproject.toml",
            "[project]\nname='app'\nversion='1.0.0'\n[tool.uv.workspace]\nmembers=['agent']\n",
        ),
        ("Cargo.toml", "[workspace]\nmembers=['agent']\n"),
    ] {
        let dir = fixture();
        let root = dir.path();
        if manifest == "Cargo.toml" {
            fs::write(
                root.join("agent/Cargo.toml"),
                "[package]\nname='agent'\nversion='0.4.1'\n",
            )
            .unwrap();
        }
        fs::write(root.join(manifest), body).unwrap();
        fails(root, &["--project", "agent", "status"], "workspace");
    }
}

#[test]
fn project_selector_is_global_and_appears_in_schema_and_completions() {
    let dir = fixture();
    let root = dir.path();
    succeeds(root, &["status", "--project", "agent"]);
    let schema = succeeds(root, &["schema"]);
    assert!(schema.contains("--project"));
    let completions = succeeds(root, &["completions", "bash"]);
    assert!(completions.contains("--project"));
    let config: serde_json::Value =
        serde_json::from_str(&succeeds(root, &["--project", "agent", "config", "show"])).unwrap();
    assert_eq!(config["selection"]["tag_prefix"], "agent-v");
    assert_eq!(
        config["verify"]["targets"],
        serde_json::json!(["tag", "pypi"])
    );
}

#[test]
fn resume_finishes_secondary_version_files_left_at_previous_version() {
    let dir = fixture();
    let root = dir.path();
    fs::write(root.join("agent/version.py"), "VERSION = \"0.4.1\"\n").unwrap();
    let config = fs::read_to_string(root.join("agent/vership.toml")).unwrap()
        + "\n[[version_files]]\nglob='version.py'\nsearch='{prev}'\nreplace='{version}'\n";
    fs::write(root.join("agent/vership.toml"), config).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: version reference"]);
    fs::write(
        root.join("agent/pyproject.toml"),
        "[project]\nname='app-agent'\nversion='0.4.2'\n",
    )
    .unwrap();
    succeeds(root, &["--project", "agent", "resume", "--no-push"]);
    assert!(git(root, &["show", "agent-v0.4.2:agent/version.py"]).contains("0.4.2"));
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn older_manifest_cannot_release_below_a_higher_selected_tag() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["tag", "agent-v0.7.0"]);
    fails(
        root,
        &["--project", "agent", "bump", "patch", "--prepare"],
        "greater semantic precedence",
    );
    assert!(
        fs::read_to_string(root.join("agent/pyproject.toml"))
            .unwrap()
            .contains("0.4.1")
    );
}

#[test]
fn amending_a_prepared_commit_requires_explicit_refresh() {
    let dir = fixture();
    let root = dir.path();
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    fs::write(root.join("agent/amended.py"), "new content").unwrap();
    git(root, &["add", "agent/amended.py"]);
    git(root, &["commit", "--amend", "--no-edit"]);
    fails(
        root,
        &["--project", "agent", "release", "--no-push"],
        "HEAD advanced",
    );
    succeeds(root, &["--project", "agent", "release", "--prepare"]);
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
}

#[test]
fn refresh_adds_new_project_changes_to_the_prepared_notes() {
    let dir = fixture();
    let root = dir.path();
    commit(root, "agent/first.py", "first", "feat: original feature");
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    commit(
        root,
        "agent/second.py",
        "second",
        "fix: added after preparation",
    );
    succeeds(root, &["--project", "agent", "release", "--prepare"]);
    let log = fs::read_to_string(root.join("agent/CHANGELOG.md")).unwrap();
    assert_eq!(log.matches("original feature").count(), 1);
    assert_eq!(log.matches("added after preparation").count(), 1);
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
}

#[test]
fn a_missing_prepared_section_cannot_replay_bump_hooks() {
    let dir = fixture();
    let root = dir.path();
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    commit(
        root,
        "agent/CHANGELOG.md",
        "# Changelog\n",
        "docs: accidentally removed section",
    );
    fails(
        root,
        &["--project", "agent", "release", "--prepare"],
        "prepared release agent-v0.4.2 is incomplete",
    );
    fails(
        root,
        &["--project", "agent", "bump", "patch", "--prepare"],
        "already prepared",
    );
    assert!(
        fs::read_to_string(root.join("agent/pyproject.toml"))
            .unwrap()
            .contains("0.4.2")
    );
}

#[test]
fn another_projects_unfinished_release_is_not_pushed_implicitly() {
    let dir = fixture();
    let bare = TempDir::new().unwrap();
    let root = dir.path();
    git(bare.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "--tags"]);
    let remote_before = git(bare.path(), &["rev-parse", "main"]);
    succeeds(root, &["bump", "patch", "--prepare"]);
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    fails(
        root,
        &["--project", "agent", "release"],
        "another unfinished release",
    );
    assert_eq!(git(bare.path(), &["rev-parse", "main"]), remote_before);
    assert!(!git(bare.path(), &["tag"]).contains("agent-v0.4.2"));
}

#[cfg(unix)]
#[test]
fn root_version_file_symlink_cannot_write_inside_an_independent_package() {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir(root.join("agent/docs")).unwrap();
    fs::write(root.join("agent/docs/version.txt"), "1.0.0").unwrap();
    std::os::unix::fs::symlink("agent/docs", root.join("docs")).unwrap();
    let config = fs::read_to_string(root.join("vership.toml")).unwrap()
        + "\n[[version_files]]\nglob='docs/version.txt'\nsearch='{prev}'\nreplace='{version}'\n";
    fs::write(root.join("vership.toml"), config).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: alias"]);
    fails(
        root,
        &["bump", "patch", "--prepare"],
        "another registered project",
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/docs/version.txt")).unwrap(),
        "1.0.0"
    );
}

#[test]
fn merge_of_child_changes_does_not_claim_main_only_changes_in_root_history() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["checkout", "-b", "child-work"]);
    commit(root, "agent/code.py", "feature", "feat: child branch");
    git(root, &["checkout", "main"]);
    commit(root, "app.py", "main", "fix: main behavior");
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "child-work",
            "-m",
            "feat: merge child work",
        ],
    );
    let root_status = status(root, false).to_string();
    let child_status = status(root, true).to_string();
    assert!(!root_status.contains("merge child work"), "{root_status}");
    assert!(child_status.contains("merge child work"), "{child_status}");
}

#[test]
fn interrupted_go_preparation_resumes_from_its_selected_changelog() {
    let dir = fixture();
    let root = dir.path();
    fs::remove_file(root.join("agent/pyproject.toml")).unwrap();
    fs::write(
        root.join("agent/go.mod"),
        "module example.invalid/agent\n\ngo 1.23\n",
    )
    .unwrap();
    fs::write(
        root.join("agent/vership.toml"),
        "[project]\ntype='go'\n[checks]\nlint=false\ntests=false\n[verify]\ntargets=['tag']\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: Go package"]);
    // Simulate an interrupted bump after changelog mutation and before commit.
    fs::write(
        root.join("agent/CHANGELOG.md"),
        "# Changelog\n\n## [0.4.2] - 2026-10-16\n\n### Added\n\n- Prepared notes\n",
    )
    .unwrap();
    succeeds(root, &["--project", "agent", "resume", "--no-push"]);
    assert_eq!(status(root, true)["latest_tag"], "agent-v0.4.2");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn dot_relative_project_paths_keep_their_commit_history() {
    let dir = fixture();
    let root = dir.path();
    let config = fs::read_to_string(root.join("vership.toml"))
        .unwrap()
        .replace("path = 'agent'", "path = './agent'")
        .replace("['agent', 'shared']", "['./agent', './shared']");
    commit(root, "vership.toml", &config, "chore: path spelling");
    commit(
        root,
        "agent/code.py",
        "feature",
        "feat: visible through normalized path",
    );
    assert_eq!(status(root, true)["unreleased_commits"], 1);
}

#[test]
fn a_preparation_without_its_local_identity_requires_refresh() {
    let dir = fixture();
    let root = dir.path();
    succeeds(root, &["--project", "agent", "bump", "patch", "--prepare"]);
    git(
        root,
        &["update-ref", "-d", "refs/vership/prepared/agent-v0.4.2"],
    );
    fails(
        root,
        &["--project", "agent", "release", "--no-push"],
        "preparation identity is missing",
    );
    succeeds(root, &["--project", "agent", "release", "--prepare"]);
    succeeds(root, &["--project", "agent", "release", "--no-push"]);
}

#[cfg(unix)]
#[test]
fn artifact_stdout_cannot_follow_a_new_symlink_into_an_independent_package() {
    let dir = fixture();
    let root = dir.path();
    fs::write(root.join("agent/preserved.txt"), "untouched").unwrap();
    let config = fs::read_to_string(root.join("vership.toml")).unwrap()
        + "\n[[artifacts]]\ncommand='ln -s agent/preserved.txt output.txt; printf overwrite'\noutput='output.txt'\n";
    fs::write(root.join("vership.toml"), config).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: artifact alias"]);
    fails(
        root,
        &["bump", "patch", "--prepare"],
        "another registered project",
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/preserved.txt")).unwrap(),
        "untouched"
    );
}
