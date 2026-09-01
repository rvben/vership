use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

/// Run a git command in `dir`, asserting it succeeds.
fn git(dir: &Path, args: &[&str]) {
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

/// Initialize a hermetic git repo on `main` with signing disabled so the real
/// commit/tag steps in `vership bump` run without external prompts.
fn init_repo(dir: &Path) {
    git(dir, &["init"]);
    git(dir, &["checkout", "-b", "main"]);
    git(dir, &["config", "user.email", "test@test.com"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "tag.gpgsign", "false"]);
}

/// Drive the real `vership` binary through a release that should promote a
/// curated `## [Unreleased]` section. Exercises the production code path end to
/// end: project detection, version bump, changelog integration, commit, tag.
#[test]
fn bump_promotes_curated_unreleased_section() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    init_repo(root);

    // Gradle project: the settings script triggers detection, the properties
    // file holds the version that gets bumped.
    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        root.join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.5\n",
    )
    .unwrap();

    let changelog = "\
# Changelog

## [Unreleased]

### Fixed

- hand written fix that must survive promotion

## [0.1.5] - 2026-05-01

### Added

- prior release entry
";
    fs::write(root.join("CHANGELOG.md"), changelog).unwrap();

    git(
        root,
        &[
            "add",
            "settings.gradle.kts",
            "gradle.properties",
            "CHANGELOG.md",
        ],
    );
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.1.5", "-m", "v0.1.5"]);

    // A real change to release since the last tag.
    fs::write(root.join("source.txt"), "change").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: real bug fix since release"]);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success()
        .get_output()
        .clone();

    // Gap: the status line must reflect that curated content was promoted,
    // not claim a changelog was generated from commits.
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("Promoted curated Unreleased notes (1 generated entries replaced)"),
        "expected promotion status message, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("Generated changelog"),
        "promotion must not be reported as generation, got stderr:\n{stderr}"
    );

    // The curated content landed under the new release on disk.
    let written = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    let unreleased = written.find("## [Unreleased]").expect("unreleased heading");
    let new_release = written.find("## [0.1.6]").expect("new release heading");
    let prior = written.find("## [0.1.5]").expect("prior release heading");
    assert!(unreleased < new_release, "fresh [Unreleased] sits on top");
    assert!(new_release < prior, "new release sits above the prior one");
    assert!(
        written.contains("- hand written fix that must survive promotion"),
        "curated entry must carry into the release"
    );
    assert_eq!(
        written.matches("## [Unreleased]").count(),
        1,
        "exactly one [Unreleased] heading remains"
    );

    // The version file was bumped through the real path.
    let props = fs::read_to_string(root.join("gradle.properties")).unwrap();
    assert!(
        props.contains("pluginVersion=0.1.6"),
        "gradle.properties bumped, got:\n{props}"
    );
}

fn setup_gradle_release(root: &Path, changelog: &str) {
    init_repo(root);
    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.5\n").unwrap();
    fs::write(root.join("CHANGELOG.md"), changelog).unwrap();
    git(
        root,
        &[
            "add",
            "settings.gradle.kts",
            "gradle.properties",
            "CHANGELOG.md",
        ],
    );
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.1.5", "-m", "v0.1.5"]);
    fs::write(root.join("source.txt"), "change").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: release fix"]);
}

#[test]
fn changelog_preview_uses_requested_level_and_curated_unbracketed_notes() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(
        root,
        "# Changelog\n\n## Unreleased\n\n### Added\n\n- curated feature\n\n## [0.1.5] - 2026-05-01\n",
    );

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "minor"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## [0.2.0]"), "got:\n{stdout}");
    assert!(stdout.contains("- curated feature"), "got:\n{stdout}");
    assert!(!stdout.contains("- release fix"), "got:\n{stdout}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("1 generated entries replaced"));
    assert!(
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .unwrap()
            .stdout
            .is_empty(),
        "preview must not mutate the repository"
    );
}

#[test]
fn changelog_preview_matches_an_interrupted_bump_target() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.6\n").unwrap();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "patch"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## [0.1.6]"), "got:\n{stdout}");
    assert!(!stdout.contains("## [0.1.7]"), "got:\n{stdout}");
}

#[test]
fn changelog_preview_shows_the_exact_curated_prepared_section() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(
        root,
        "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- curated release note\n",
    );

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--prepare"])
        .assert()
        .success();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "patch"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## [0.1.6]"), "got:\n{stdout}");
    assert!(stdout.contains("- curated release note"), "got:\n{stdout}");
    assert!(!stdout.contains("- release fix"), "got:\n{stdout}");
    assert_eq!(stdout.matches("## [0.1.6]").count(), 1);
}

#[test]
fn git_breaking_footer_reaches_changelog_without_buffering_full_body() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(root.join("protocol.txt"), "changed").unwrap();
    git(root, &["add", "protocol.txt"]);
    git(
        root,
        &[
            "commit",
            "-m",
            "chore: change protocol",
            "-m",
            "BREAKING CHANGE: clients must reconnect",
        ],
    );

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Generated changelog (2 entries)"),
        "got:\n{stderr}"
    );
    assert!(stderr.contains("### Breaking Changes"), "got:\n{stderr}");
    assert!(stderr.contains("change protocol"), "got:\n{stderr}");
}

#[test]
fn include_mode_counts_non_conventional_changelog_entries() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(
        root.join("vership.toml"),
        "[changelog]\nunconventional = \"include\"\n",
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure changelog"]);
    fs::write(root.join("notes.txt"), "documented").unwrap();
    git(root, &["add", "notes.txt"]);
    git(root, &["commit", "-m", "Document operational behavior"]);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Generated changelog (2 entries)"),
        "got:\n{stderr}"
    );
    assert!(stderr.contains("### Other"), "got:\n{stderr}");
    assert!(
        stderr.contains("Document operational behavior"),
        "got:\n{stderr}"
    );
}

#[test]
fn unreadable_changelog_bytes_fail_without_overwrite() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    let invalid = [0xff, 0xfe, 0xfd];
    fs::write(root.join("CHANGELOG.md"), invalid).unwrap();

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "patch"])
        .assert()
        .failure();
    assert_eq!(fs::read(root.join("CHANGELOG.md")).unwrap(), invalid);
}

#[test]
fn preflight_checks_the_requested_release_tag() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    git(root, &["tag", "-a", "v0.2.0", "-m", "occupied"]);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["preflight", "minor"])
        .assert()
        .failure()
        .get_output()
        .clone();

    assert!(String::from_utf8_lossy(&output.stderr).contains("Tag v0.2.0 already exists"));
}

#[test]
fn preflight_rejects_untracked_files_by_default() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(root.join("forgotten.txt"), "important").unwrap();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .arg("preflight")
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Untracked files detected"),
        "got:\n{stderr}"
    );
    assert!(stderr.contains("forgotten.txt"), "got:\n{stderr}");
}

#[test]
fn preflight_can_explicitly_allow_untracked_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(
        root.join("vership.toml"),
        "[checks]\nallow_untracked = true\n",
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure release checks"]);
    fs::write(root.join("scratch.txt"), "intentional").unwrap();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .arg("preflight")
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("untracked path(s) explicitly allowed")
    );
}

#[test]
fn legacy_library_preflight_ignores_untracked_files_and_remote_access() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(root.join("scratch.txt"), "not part of the release").unwrap();
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://invalid.invalid/never.git",
        ],
    );
    let project = vership::project::detect(root, None).unwrap();
    let options = vership::checks::CheckOptions {
        expected_branch: "main".to_string(),
        run_lint: false,
        run_tests: false,
        lint_command: None,
        test_command: None,
        allow_uncommitted: false,
    };

    vership::checks::run_preflight(root, "v0.1.6", project.as_ref(), &options)
        .expect("the stable library wrapper remains local-only and tracked-only");
}

#[test]
fn resume_still_rejects_unrelated_untracked_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.6\n").unwrap();
    fs::write(root.join("forgotten.txt"), "not part of the release").unwrap();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["resume", "--skip-checks", "--no-push"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Untracked files detected"),
        "got:\n{stderr}"
    );
    assert!(stderr.contains("forgotten.txt"), "got:\n{stderr}");
    assert!(!tag_exists(root, "v0.1.6"));
}

#[test]
fn failed_custom_checks_preserve_diagnostics() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(
        root.join("vership.toml"),
        "[checks]\nlint = false\ntest_command = \"printf 'specific failure\\n'; exit 9\"\n",
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure checks"]);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .arg("preflight")
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(
        output.stdout.is_empty(),
        "child diagnostics must not contaminate command stdout"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("specific failure"));
}

#[test]
fn release_hook_and_artifact_stdout_is_routed_to_stderr() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    fs::write(
        root.join("vership.toml"),
        "[hooks]\npre-bump = \"printf hook-diagnostic\"\n\n[[artifacts]]\ncommand = \"printf artifact-diagnostic\"\n",
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(
        root,
        &["commit", "-m", "chore: configure release diagnostics"],
    );

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success()
        .get_output()
        .clone();

    assert!(output.stdout.is_empty(), "release stdout must remain clean");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hook-diagnostic"), "got:\n{stderr}");
    assert!(stderr.contains("artifact-diagnostic"), "got:\n{stderr}");
}

#[test]
fn prepare_creates_a_reviewable_commit_without_a_tag() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--prepare"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&output.stderr).contains("Prepared release commit for v0.1.6"));

    assert!(!tag_exists(root, "v0.1.6"));
    let message = Command::new("git")
        .args(["log", "-1", "--format=%B"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&message.stdout).contains("Vership-Release: v0.1.6"));
    assert!(
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );

    let preview = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "patch"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview = String::from_utf8_lossy(&preview);
    assert!(preview.contains("## [0.1.6]"), "got:\n{preview}");
    assert!(!preview.contains("## [0.1.7]"), "got:\n{preview}");

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks", "--no-push"])
        .assert()
        .success();
    assert!(tag_exists(root, "v0.1.6"));
}

#[test]
fn completing_a_prepared_release_does_not_replay_bump_hooks() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    let marker_dir = TempDir::new().unwrap();
    let marker = marker_dir.path().join("hook-runs");
    fs::write(
        root.join("vership.toml"),
        format!(
            "[hooks]\npre-bump = \"printf x >> '{}'\"\n",
            marker.display()
        ),
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure release hook"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--prepare"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks", "--no-push"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        "x",
        "completing the release must not replay pre-bump hooks"
    );
    assert!(tag_exists(root, "v0.1.6"));
}

#[test]
fn bump_retries_an_unpublished_local_tag_after_push_failure() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "v0.1.5"]);

    let marker_dir = TempDir::new().unwrap();
    let marker = marker_dir.path().join("allow-second-push");
    fs::write(
        root.join("vership.toml"),
        format!(
            "[hooks]\npre-push = \"if [ -f '{}' ]; then exit 0; else touch '{}'; exit 7; fi\"\n",
            marker.display(),
            marker.display()
        ),
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure retry hook"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks"])
        .assert()
        .failure();
    assert!(tag_exists(root, "v0.1.6"));
    assert!(!vership::git::remote_tag_exists(root, "v0.1.6").unwrap());

    let preview = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["changelog", "patch"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview = String::from_utf8_lossy(&preview);
    assert!(preview.contains("## [0.1.6]"), "got:\n{preview}");
    assert!(!preview.contains("## [0.1.7]"), "got:\n{preview}");

    let tag_before_dry_run = Command::new("git")
        .args(["rev-parse", "refs/tags/v0.1.6"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;
    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--dry-run"])
        .assert()
        .success();
    let tag_after_dry_run = Command::new("git")
        .args(["rev-parse", "refs/tags/v0.1.6"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(tag_before_dry_run, tag_after_dry_run);
    assert!(!vership::git::remote_tag_exists(root, "v0.1.6").unwrap());

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks"])
        .assert()
        .success();
    assert!(vership::git::remote_tag_exists(root, "v0.1.6").unwrap());
    let remote_branch = Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(remote.path())
        .output()
        .unwrap()
        .stdout;
    let remote_tag = Command::new("git")
        .args(["rev-parse", "v0.1.6^{}"])
        .current_dir(remote.path())
        .output()
        .unwrap()
        .stdout;
    assert_eq!(remote_branch, remote_tag);
}

#[test]
fn release_without_a_new_commit_retries_an_unpublished_tag() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "origin", "main", "v0.1.5"]);

    let marker_dir = TempDir::new().unwrap();
    let marker = marker_dir.path().join("allow-second-push");
    fs::write(
        root.join("vership.toml"),
        format!(
            "[hooks]\npre-push = \"if [ -f '{}' ]; then exit 0; else touch '{}'; exit 7; fi\"\n",
            marker.display(),
            marker.display()
        ),
    )
    .unwrap();
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.6\n").unwrap();
    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n\n## [0.1.6] - 2026-09-01\n\n- manual release\n",
    )
    .unwrap();
    git(
        root,
        &["add", "vership.toml", "gradle.properties", "CHANGELOG.md"],
    );
    git(root, &["commit", "-m", "chore: manually prepare release"]);
    let head_before = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks"])
        .assert()
        .failure();
    assert!(tag_exists(root, "v0.1.6"));

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks"])
        .assert()
        .success();
    let head_after = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(
        head_before, head_after,
        "retry must not create an empty commit"
    );
    assert!(vership::git::remote_tag_exists(root, "v0.1.6").unwrap());
}

#[test]
fn manual_release_state_is_not_mistaken_for_prepare() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n");
    let marker_dir = TempDir::new().unwrap();
    let marker = marker_dir.path().join("hook-runs");
    fs::write(
        root.join("vership.toml"),
        format!(
            "[hooks]\npre-bump = \"printf x >> '{}'\"\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.6\n").unwrap();
    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n\n## [0.1.6] - 2026-09-01\n\n- manual notes\n",
    )
    .unwrap();
    git(
        root,
        &["add", "vership.toml", "gradle.properties", "CHANGELOG.md"],
    );
    git(root, &["commit", "-m", "chore: manually prepare release"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks", "--no-push"])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
    assert!(tag_exists(root, "v0.1.6"));
}

#[test]
fn pre_bump_hook_changelog_edits_are_preserved() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_gradle_release(root, "# Changelog\n\n## [Unreleased]\n");
    fs::write(
        root.join("vership.toml"),
        "[hooks]\npre-bump = \"printf '\\n### Fixed\\n\\n- hook-curated note\\n' >> CHANGELOG.md\"\n",
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(root, &["commit", "-m", "chore: configure changelog hook"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success();

    let changelog = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    let released = vership::changelog::extract_section(&changelog, "0.1.6").unwrap();
    assert!(released.contains("- hook-curated note"), "got:\n{released}");
}

/// Drive the real `vership` binary against a CHANGELOG that carries bottom
/// link-reference definitions. The inline-linked headers vership writes are
/// self-contained, so the stale `[Unreleased]:` / `[x.y.z]:` refs must be
/// stripped on disk while prose refs survive. Exercises the production path.
#[test]
fn bump_strips_stale_changelog_link_refs() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    init_repo(root);

    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        root.join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.5\n",
    )
    .unwrap();

    let changelog = "\
# Changelog

## [Unreleased]

### Fixed

- curated fix

## [0.1.5] - 2026-05-01

### Added

- prior release entry

[Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/o/r/releases/tag/v0.1.5
[contributing]: https://github.com/o/r/blob/main/CONTRIBUTING.md
";
    fs::write(root.join("CHANGELOG.md"), changelog).unwrap();

    git(
        root,
        &[
            "add",
            "settings.gradle.kts",
            "gradle.properties",
            "CHANGELOG.md",
        ],
    );
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.1.5", "-m", "v0.1.5"]);

    fs::write(root.join("source.txt"), "change").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: real bug fix since release"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success();

    let written = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();

    // Promotion still works through the real path.
    assert!(written.contains("- curated fix"));
    assert!(written.contains("## [0.1.6]"));

    // Stale version link-reference definitions are stripped on disk.
    assert!(
        !written.contains("[Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD"),
        "stale [Unreleased] ref must be gone, got:\n{written}"
    );
    assert!(
        !written.contains("[0.1.5]: https://github.com/o/r/releases/tag/v0.1.5"),
        "version ref must be gone, got:\n{written}"
    );

    // Prose link-reference definitions survive.
    assert!(
        written.contains("[contributing]: https://github.com/o/r/blob/main/CONTRIBUTING.md"),
        "non-version ref must be preserved, got:\n{written}"
    );

    // Exactly one [Unreleased] heading remains and no trailing blank cruft.
    assert_eq!(written.matches("## [Unreleased]").count(), 1);
    assert!(written.ends_with("CONTRIBUTING.md\n"));
    assert!(!written.ends_with("\n\n"));
}

/// Bring a repo to the retag state: a completed bump (version + changelog
/// committed, tag created), then the tag deleted because the release needed
/// a fix before publishing. Returns the commit count at that state.
fn setup_committed_release_without_tag(root: &Path) -> usize {
    init_repo(root);
    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        root.join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.5\n",
    )
    .unwrap();
    git(root, &["add", "settings.gradle.kts", "gradle.properties"]);
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.1.5", "-m", "v0.1.5"]);

    fs::write(root.join("source.txt"), "change").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: real bug fix since release"]);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success();

    // The release was caught before publishing: delete the tag so it can be
    // re-created on a corrected HEAD (the documented retag flow).
    git(root, &["tag", "-d", "v0.1.6"]);

    commit_count(root)
}

fn commit_count(root: &Path) -> usize {
    let output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("count parses")
}

fn tag_exists(root: &Path, tag: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/tags/{tag}")])
        .current_dir(root)
        .output()
        .expect("git runs")
        .status
        .success()
}

/// Retag flow via `release`: everything is already committed and only the tag
/// is missing. The run must converge (exit 0, tag created) without inventing
/// a new commit or failing on the empty commit.
#[test]
fn release_retags_when_release_commit_already_exists() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let commits_before = setup_committed_release_without_tag(root);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks", "--no-push"])
        .assert()
        .success()
        .get_output()
        .clone();

    assert!(tag_exists(root, "v0.1.6"), "tag must be re-created");
    assert_eq!(
        commit_count(root),
        commits_before,
        "no new commit when the release commit already exists"
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("Nothing to commit"),
        "skip must be reported, got stderr:\n{stderr}"
    );
}

/// Same state via `resume`: an interrupted run whose commit landed but whose
/// tag step never ran. Resume must finish the tag step.
#[test]
fn resume_finishes_tagging_when_commit_already_landed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let commits_before = setup_committed_release_without_tag(root);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["resume", "--skip-checks", "--no-push"])
        .assert()
        .success();

    assert!(tag_exists(root, "v0.1.6"), "tag must be created");
    assert_eq!(
        commit_count(root),
        commits_before,
        "no new commit when resuming after the commit landed"
    );
}

/// The fix-on-top retag flow: after deleting the tag, a correction commit is
/// added. The release must tag the corrected HEAD without a fresh release
/// commit (the changelog and version are already in history).
#[test]
fn release_retags_corrected_head_after_fix_commit() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_committed_release_without_tag(root);

    let marker_dir = TempDir::new().unwrap();
    let hook_marker = marker_dir.path().join("replayed-hook");
    fs::write(
        root.join("vership.toml"),
        format!(
            "[hooks]\npre-bump = \"touch '{}'\"\n\n[[artifacts]]\ncommand = \"printf artifact\"\noutput = \"artifact.txt\"\n",
            hook_marker.display()
        ),
    )
    .unwrap();
    git(root, &["add", "vership.toml"]);
    git(
        root,
        &["commit", "-m", "chore: configure release side effects"],
    );

    fs::write(root.join("source.txt"), "corrected").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: correct the release"]);
    let commits_before = commit_count(root);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["release", "--skip-checks", "--no-push"])
        .assert()
        .success();

    assert!(tag_exists(root, "v0.1.6"), "tag must be re-created");
    assert_eq!(commit_count(root), commits_before, "no extra commit");
    assert!(
        !hook_marker.exists(),
        "pre-bump hook must not replay after a prepared release commit"
    );
    assert!(
        !root.join("artifact.txt").exists(),
        "artifact generators must not replay after a prepared release commit"
    );

    // The tag points at the corrected HEAD, not the original bump commit.
    let tag_target = Command::new("git")
        .args(["rev-parse", "v0.1.6^{commit}"])
        .current_dir(root)
        .output()
        .expect("git runs");
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git runs");
    assert_eq!(
        String::from_utf8_lossy(&tag_target.stdout),
        String::from_utf8_lossy(&head.stdout),
        "tag must point at the corrected HEAD"
    );
}

/// Stand up a committed Ansible collection repo (galaxy.yml only) with one
/// unreleased conventional commit, ready for a bump.
fn setup_ansible_collection(root: &Path, galaxy: &str) {
    init_repo(root);
    fs::write(root.join("galaxy.yml"), galaxy).unwrap();
    git(root, &["add", "galaxy.yml"]);
    git(root, &["commit", "-m", "chore: initial"]);

    fs::write(root.join("roles.txt"), "a role").unwrap();
    git(root, &["add", "roles.txt"]);
    git(root, &["commit", "-m", "feat: add a role"]);
}

/// `vership status --output json` on a galaxy.yml-only repo reports the
/// Ansible-collection type, the version from galaxy.yml, and the FQCN.
#[test]
fn ansible_status_reports_type_version_and_fqcn() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    setup_ansible_collection(root, "namespace: hda\nname: platform\nversion: \"0.0.2\"\n");

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["status", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status emits valid json");
    assert_eq!(json["project_type"], "Ansible Collection");
    assert_eq!(json["current_version"], "0.0.2");
    assert_eq!(json["name"], "hda.platform");
}

/// `bump patch --dry-run` previews 0.0.2 -> 0.0.3 and mutates nothing: the
/// galaxy.yml is untouched and no tag is created.
#[test]
fn ansible_bump_patch_dry_run_makes_no_changes() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let galaxy = "namespace: hda\nname: platform\nversion: \"0.0.2\"\n";
    setup_ansible_collection(root, galaxy);

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--dry-run", "--skip-checks", "--no-push"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("0.0.2") && stderr.contains("0.0.3"),
        "dry run previews the version transition, got:\n{stderr}"
    );
    assert!(stderr.contains("Dry run"), "dry run is announced");

    // Nothing changed on disk or in git.
    assert_eq!(fs::read_to_string(root.join("galaxy.yml")).unwrap(), galaxy);
    assert!(!root.join("CHANGELOG.md").exists());
    assert!(!tag_exists(root, "v0.0.3"));
}

/// A real `bump patch` rewrites only the version line (quotes preserved),
/// generates CHANGELOG.md, commits, and tags v0.0.3 (no push).
#[test]
fn ansible_bump_patch_rewrites_single_line_and_tags() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let galaxy = "# managed by vership\nnamespace: hda\nname: platform\nversion: \"0.0.2\"\nreadme: README.md\n";
    setup_ansible_collection(root, galaxy);

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks", "--no-push"])
        .assert()
        .success();

    // Only the version line changed; quoting, comments, and key order intact.
    let written = fs::read_to_string(root.join("galaxy.yml")).unwrap();
    assert_eq!(
        written,
        "# managed by vership\nnamespace: hda\nname: platform\nversion: \"0.0.3\"\nreadme: README.md\n"
    );

    // Changelog generated and tag created with the v prefix.
    let changelog = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("## [0.0.3]"));
    assert!(tag_exists(root, "v0.0.3"), "tag v0.0.3 must be created");
}

/// Resuming an interrupted Ansible bump (galaxy.yml written but never
/// committed) must commit the bumped manifest so the tag points at a tree whose
/// galaxy.yml equals the tag. A collection is installed by git ref, so a tag
/// whose galaxy.yml carries the prior version would ship the wrong version.
#[test]
fn ansible_resume_commits_uncommitted_manifest_so_tag_matches() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    init_repo(root);
    fs::write(
        root.join("galaxy.yml"),
        "namespace: hda\nname: platform\nversion: \"0.0.2\"\n",
    )
    .unwrap();
    git(root, &["add", "galaxy.yml"]);
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.0.2", "-m", "v0.0.2"]);
    fs::write(root.join("roles.txt"), "a role").unwrap();
    git(root, &["add", "roles.txt"]);
    git(root, &["commit", "-m", "feat: add a role"]);

    // Simulate an interrupted bump: the version is written to disk but the
    // commit never happened, leaving galaxy.yml dirty at the new version.
    fs::write(
        root.join("galaxy.yml"),
        "namespace: hda\nname: platform\nversion: \"0.0.3\"\n",
    )
    .unwrap();

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["resume", "--skip-checks", "--no-push"])
        .assert()
        .success();

    assert!(tag_exists(root, "v0.0.3"), "tag v0.0.3 must be created");

    // The tagged tree's galaxy.yml must carry the released version.
    let tagged = Command::new("git")
        .args(["show", "v0.0.3:galaxy.yml"])
        .current_dir(root)
        .output()
        .expect("git runs");
    let tagged = String::from_utf8_lossy(&tagged.stdout);
    assert!(
        tagged.contains("version: \"0.0.3\""),
        "tagged galaxy.yml must be at 0.0.3, got:\n{tagged}"
    );

    // No bumped manifest left stranded in the working tree.
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "working tree must be clean after resume"
    );
}

/// `minor` and `major` bumps from 0.0.2 produce 0.1.0 and 1.0.0.
#[test]
fn ansible_bump_minor_and_major() {
    for (level, expected_version, expected_tag) in
        [("minor", "0.1.0", "v0.1.0"), ("major", "1.0.0", "v1.0.0")]
    {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        setup_ansible_collection(root, "namespace: hda\nname: platform\nversion: \"0.0.2\"\n");

        AssertCommand::cargo_bin("vership")
            .unwrap()
            .current_dir(root)
            .args(["bump", level, "--skip-checks", "--no-push"])
            .assert()
            .success();

        let written = fs::read_to_string(root.join("galaxy.yml")).unwrap();
        assert!(
            written.contains(&format!("version: \"{expected_version}\"")),
            "{level} bump should yield {expected_version}, got:\n{written}"
        );
        assert!(
            tag_exists(root, expected_tag),
            "tag {expected_tag} must exist"
        );
    }
}

/// Resuming an interrupted bump must stage configured `[[version_files]]`, not
/// just the manifest. An interrupted run leaves a version_file (here README)
/// bumped on disk but uncommitted; if resume omits it, the tagged tree carries
/// the manifest at the new version but README at the old one - an internally
/// inconsistent release. Project type is irrelevant (the version_files step is
/// shared), so a Gradle project keeps the test free of a cargo invocation.
#[test]
fn resume_stages_configured_version_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    init_repo(root);

    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.0\n").unwrap();
    fs::write(root.join("README.md"), "Install demo (rev v0.1.0).\n").unwrap();
    fs::write(
        root.join("vership.toml"),
        "[[version_files]]\nglob = \"README.md\"\nsearch = \"v{prev}\"\nreplace = \"v{version}\"\n",
    )
    .unwrap();
    fs::write(root.join("CHANGELOG.md"), "# Changelog\n\n").unwrap();
    git(
        root,
        &[
            "add",
            "settings.gradle.kts",
            "gradle.properties",
            "README.md",
            "vership.toml",
            "CHANGELOG.md",
        ],
    );
    git(root, &["commit", "-m", "chore: initial"]);
    git(root, &["tag", "-a", "v0.1.0", "-m", "v0.1.0"]);
    fs::write(root.join("source.txt"), "change").unwrap();
    git(root, &["add", "source.txt"]);
    git(root, &["commit", "-m", "fix: real bug fix since release"]);

    // Simulate an interrupted bump: manifest, the README version_file, and the
    // changelog are written to disk at the new version but never committed.
    fs::write(root.join("gradle.properties"), "pluginVersion=0.1.1\n").unwrap();
    fs::write(root.join("README.md"), "Install demo (rev v0.1.1).\n").unwrap();
    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## [0.1.1] - 2026-01-01\n\n### Fixed\n\n- real bug fix since release\n",
    )
    .unwrap();

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["resume", "--skip-checks", "--no-push"])
        .assert()
        .success();

    assert!(tag_exists(root, "v0.1.1"), "tag v0.1.1 must be created");

    // The tagged tree's README (a configured version_file) must carry the
    // released version, not the prior one.
    let tagged = Command::new("git")
        .args(["show", "v0.1.1:README.md"])
        .current_dir(root)
        .output()
        .expect("git runs");
    let tagged = String::from_utf8_lossy(&tagged.stdout);
    assert!(
        tagged.contains("v0.1.1"),
        "tagged README must be at v0.1.1, got:\n{tagged}"
    );

    // No bumped version file left stranded in the working tree.
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "working tree must be clean after resume"
    );
}
