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
        stderr.contains("Promoted [Unreleased] section"),
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
