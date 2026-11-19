//! Real CLI contracts for a root project whose release tags do not start with `v`.
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
fn tags(root: &Path) -> Vec<String> {
    git(root, &["tag", "--list"])
        .lines()
        .map(str::to_owned)
        .collect()
}
fn status(root: &Path) -> serde_json::Value {
    serde_json::from_str(&succeeds(root, &["status"])).unwrap()
}

/// A Python project at 1.0.0 whose `[project]` table is `project` and whose
/// only release so far is tagged `released`.
fn fixture(project: &str, released: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "tag.gpgsign", "false"]);
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0.0'\n",
    )
    .unwrap();
    fs::write(
        root.join("vership.toml"),
        format!(
            "[project]\ntype = 'python'\n{project}\n[checks]\nlint = false\ntests = false\n[verify]\ntargets = ['tag']\n"
        ),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", released]);
    dir
}
fn with_remote(root: &Path) -> TempDir {
    let bare = TempDir::new().unwrap();
    git(bare.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "--tags"]);
    bare
}

#[test]
fn an_empty_prefix_releases_under_the_bare_version() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let root = dir.path();
    let bare = with_remote(root);
    assert_eq!(status(root)["latest_tag"], "1.0.0");
    commit(root, "app.py", "fix", "fix: a bug");

    succeeds(root, &["bump", "patch"]);

    assert_eq!(tags(root), ["1.0.0", "1.0.1"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    assert_eq!(git(bare.path(), &["rev-parse", "main"]), head);
    assert_eq!(git(bare.path(), &["rev-parse", "1.0.1^{}"]), head);
    assert_eq!(tags(bare.path()), ["1.0.0", "1.0.1"]);
    assert_eq!(status(root)["latest_tag"], "1.0.1");
    assert!(
        fs::read_to_string(root.join("pyproject.toml"))
            .unwrap()
            .contains("1.0.1")
    );
    succeeds(root, &["verify", "--targets", "tag"]);
}

#[test]
fn a_prepared_release_is_tagged_with_the_configured_prefix() {
    let dir = fixture("tag_prefix = 'release-'", "release-1.0.0");
    let root = dir.path();
    commit(root, "app.py", "feat", "feat: a feature");

    succeeds(root, &["bump", "minor", "--prepare"]);
    assert_eq!(tags(root), ["release-1.0.0"]);
    succeeds(root, &["release", "--no-push"]);

    assert_eq!(tags(root), ["release-1.0.0", "release-1.1.0"]);
    assert_eq!(status(root)["latest_tag"], "release-1.1.0");
}

#[test]
fn changelog_compare_links_name_the_real_tags() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
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
    commit(root, "app.py", "fix", "fix: a bug");

    let preview = succeeds(root, &["changelog"]);

    assert!(preview.contains("compare/1.0.0...1.0.1)"), "{preview}");
}

#[test]
fn tags_outside_the_configured_namespace_are_not_releases() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let root = dir.path();
    git(root, &["tag", "v9.9.9"]);
    commit(root, "app.py", "fix", "fix: a bug");

    assert_eq!(status(root)["latest_tag"], "1.0.0");
    succeeds(root, &["bump", "patch", "--no-push"]);

    assert_eq!(tags(root), ["1.0.0", "1.0.1", "v9.9.9"]);
}

#[test]
fn the_default_namespace_does_not_read_bare_tags() {
    let dir = fixture("", "v1.0.0");
    let root = dir.path();
    git(root, &["tag", "9.9.9"]);
    commit(root, "app.py", "fix", "fix: a bug");

    assert_eq!(status(root)["latest_tag"], "v1.0.0");
    succeeds(root, &["bump", "patch", "--no-push"]);

    assert_eq!(tags(root), ["9.9.9", "v1.0.0", "v1.0.1"]);
}

#[test]
fn the_effective_prefix_is_part_of_the_shown_config() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let shown: serde_json::Value =
        serde_json::from_str(&succeeds(dir.path(), &["config", "show"])).unwrap();
    assert_eq!(shown["project"]["tag_prefix"], "");

    let dir = fixture("", "v1.0.0");
    let shown: serde_json::Value =
        serde_json::from_str(&succeeds(dir.path(), &["config", "show"])).unwrap();
    assert_eq!(shown["project"]["tag_prefix"], "v");
}

#[test]
fn an_unusable_prefix_is_rejected_before_any_mutation() {
    // The last two would make every tag a version in its own right.
    for prefix in [
        "-v",
        "rel ease-",
        "bad*",
        "bad..tag",
        "bad#tag",
        "1",
        "1.2.3-",
    ] {
        let dir = fixture(&format!("tag_prefix = '{prefix}'"), "v1.0.0");
        let root = dir.path();
        commit(root, "app.py", "fix", "fix: a bug");
        fails(
            root,
            &["bump", "patch", "--no-push"],
            "invalid tag_prefix for the root project",
        );
        assert_eq!(tags(root), ["v1.0.0"], "prefix {prefix:?}");
        assert_eq!(git(root, &["status", "--porcelain"]), "");
    }
}

/// Adds an independent `agent` project released under `agent_prefix`.
fn register_agent(root: &Path, agent_prefix: &str, agent_project: &str) {
    fs::create_dir(root.join("agent")).unwrap();
    fs::write(
        root.join("agent/pyproject.toml"),
        "[project]\nname = 'app-agent'\nversion = '0.4.1'\n",
    )
    .unwrap();
    fs::write(
        root.join("agent/vership.toml"),
        format!(
            "[project]\ntype = 'python'\n{agent_project}\n[checks]\nlint = false\ntests = false\n[verify]\ntargets = ['tag']\n"
        ),
    )
    .unwrap();
    let mut config = fs::read_to_string(root.join("vership.toml")).unwrap();
    config.push_str(&format!(
        "[projects.agent]\npath = 'agent'\ntag_prefix = '{agent_prefix}'\n"
    ));
    fs::write(root.join("vership.toml"), config).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: add agent"]);
}

#[test]
fn a_bare_root_and_a_prefixed_project_release_independently() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let root = dir.path();
    register_agent(root, "agent-v", "");
    git(root, &["tag", "agent-v0.4.1"]);
    commit(root, "agent/code.py", "agent", "feat: agent feature");
    commit(root, "app.py", "root", "fix: root bug");

    succeeds(root, &["--project", "agent", "bump", "minor", "--no-push"]);
    succeeds(root, &["bump", "patch", "--no-push"]);

    assert_eq!(
        tags(root),
        ["1.0.0", "1.0.1", "agent-v0.4.1", "agent-v0.5.0"]
    );
    // What `config show` prints for the project must be loadable as its config.
    let shown: serde_json::Value =
        serde_json::from_str(&succeeds(root, &["--project", "agent", "config", "show"])).unwrap();
    assert_eq!(shown["selection"]["tag_prefix"], "agent-v");
    assert!(shown["project"].get("tag_prefix").is_none(), "{shown}");
}

#[test]
fn an_explicit_version_may_be_bare_or_spelled_as_a_tag() {
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let root = dir.path();
    let _remote = with_remote(root);
    for spelling in ["1.0.0", "v1.0.0"] {
        let report: serde_json::Value =
            serde_json::from_str(&succeeds(root, &["verify", spelling, "--targets", "tag"]))
                .unwrap();
        assert_eq!(report["targets"][0]["found"], "1.0.0", "{spelling}");
    }
    fails(root, &["verify", "one.two"], "invalid version 'one.two'");

    // A prefix that a version could also start with does not eat a bare version.
    let dir = fixture("tag_prefix = '1.'", "1.1.0.0");
    let root = dir.path();
    let _remote = with_remote(root);
    for spelling in ["1.0.0", "1.1.0.0"] {
        let report: serde_json::Value =
            serde_json::from_str(&succeeds(root, &["verify", spelling, "--targets", "tag"]))
                .unwrap();
        assert_eq!(report["version"], "1.0.0", "{spelling}");
        assert_eq!(report["targets"][0]["found"], "1.1.0.0", "{spelling}");
    }
}

#[test]
fn a_project_namespace_that_could_read_as_a_root_version_is_rejected() {
    // Under a bare root, `2024.1.0.0` is both a root-looking tag and an agent tag.
    let dir = fixture("tag_prefix = ''", "1.0.0");
    let root = dir.path();
    register_agent(root, "2024.", "");
    fails(root, &["status"], "potentially ambiguous tag namespaces");

    let dir = fixture("tag_prefix = 'agent-v'", "agent-v1.0.0");
    let root = dir.path();
    register_agent(root, "agent-v", "");
    fails(root, &["status"], "duplicate tag_prefix 'agent-v'");
}

#[test]
fn a_registered_project_takes_its_prefix_from_the_registry_only() {
    let dir = fixture("", "v1.0.0");
    let root = dir.path();
    register_agent(root, "agent-v", "tag_prefix = 'other-'");
    fails(
        root,
        &["--project", "agent", "status"],
        "set tag_prefix for project 'agent' in the repository registry",
    );
}
