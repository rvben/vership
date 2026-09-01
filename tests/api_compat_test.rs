//! Compile-time coverage for public shapes available in Vership 0.5.20.

use vership::changelog::ChangelogUpdate;
use vership::checks::CheckOptions;
use vership::cli::{BumpLevel, Command};
use vership::config::ChecksConfig;
use vership::git::Commit;

#[test]
fn patch_release_preserves_constructible_public_types() {
    let _ = Command::Bump {
        level: BumpLevel::Patch,
        dry_run: false,
        skip_checks: false,
        no_push: false,
    };
    let _ = Command::Changelog;
    let _ = Command::Preflight;
    let _ = CheckOptions {
        expected_branch: "main".into(),
        run_lint: true,
        run_tests: true,
        lint_command: None,
        test_command: None,
        allow_uncommitted: false,
    };
    let _ = ChecksConfig {
        lint: true,
        tests: true,
        lint_command: None,
        test_command: None,
    };
    let _ = ChangelogUpdate {
        content: String::new(),
        promoted: false,
    };
    let commit = Commit {
        hash: "abc".into(),
        message: "fix: subject only".into(),
    };
    assert_eq!(commit.message, commit.subject());
}
