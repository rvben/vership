use vership::changelog::{generate_changelog, parse_conventional_commit, ConventionalCommit};
use vership::git::Commit;

#[test]
fn parse_feat_with_scope() {
    let cc = parse_conventional_commit("feat(api): add user endpoint").unwrap();
    assert_eq!(cc.commit_type, "feat");
    assert_eq!(cc.scope.as_deref(), Some("api"));
    assert_eq!(cc.description, "add user endpoint");
    assert!(!cc.breaking);
}

#[test]
fn parse_fix_without_scope() {
    let cc = parse_conventional_commit("fix: correct null handling").unwrap();
    assert_eq!(cc.commit_type, "fix");
    assert!(cc.scope.is_none());
    assert_eq!(cc.description, "correct null handling");
    assert!(!cc.breaking);
}

#[test]
fn parse_breaking_with_bang() {
    let cc = parse_conventional_commit("feat!: remove deprecated API").unwrap();
    assert!(cc.breaking);
    assert_eq!(cc.commit_type, "feat");
}

#[test]
fn parse_breaking_with_scope_and_bang() {
    let cc = parse_conventional_commit("fix(auth)!: require token refresh").unwrap();
    assert!(cc.breaking);
    assert_eq!(cc.scope.as_deref(), Some("auth"));
}

#[test]
fn parse_non_conventional_returns_none() {
    let result = parse_conventional_commit("Update README");
    assert!(result.is_none());
}

#[test]
fn parse_chore_excluded() {
    let cc = parse_conventional_commit("chore: bump deps").unwrap();
    assert_eq!(cc.commit_type, "chore");
    // Parsing succeeds, but excluded from changelog by the generator
}

#[test]
fn parse_merge_commit_returns_none() {
    let result = parse_conventional_commit("Merge branch 'main' into feature");
    assert!(result.is_none());
}

#[test]
fn changelog_groups_by_type() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: add export".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "fix: null check".into(),
        },
        Commit {
            hash: "ghi9012".into(),
            message: "feat(cli): add --verbose".into(),
        },
        Commit {
            hash: "jkl3456".into(),
            message: "chore: bump deps".into(),
        },
    ];

    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), None);
    assert!(changelog.contains("### Added"));
    assert!(changelog.contains("### Fixed"));
    assert!(!changelog.contains("chore"));
    assert!(changelog.contains("add export"));
    assert!(changelog.contains("null check"));
    assert!(changelog.contains("**cli**"));
}

#[test]
fn changelog_breaking_changes_at_top() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat!: remove legacy API".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "feat: add new API".into(),
        },
    ];

    let changelog = generate_changelog(&commits, "2.0.0", Some("1.0.0"), None);
    let breaking_pos = changelog.find("### Breaking Changes").unwrap();
    let added_pos = changelog.find("### Added").unwrap();
    assert!(breaking_pos < added_pos);
}

#[test]
fn changelog_includes_commit_hash_links() {
    let commits = vec![Commit {
        hash: "abc1234def5678".into(),
        message: "feat: add feature".into(),
    }];

    let base_url = "https://github.com/rvben/vership";
    let changelog = generate_changelog(&commits, "0.1.0", None, Some(base_url));
    assert!(changelog.contains(
        "[abc1234](https://github.com/rvben/vership/commit/abc1234def5678)"
    ));
}

#[test]
fn changelog_includes_compare_link() {
    let commits = vec![Commit {
        hash: "abc1234".into(),
        message: "feat: add feature".into(),
    }];

    let base_url = "https://github.com/rvben/vership";
    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), Some(base_url));
    assert!(changelog
        .contains("(https://github.com/rvben/vership/compare/v0.1.0...v0.2.0)"));
}

#[test]
fn changelog_no_links_without_remote() {
    let commits = vec![Commit {
        hash: "abc1234".into(),
        message: "feat: add feature".into(),
    }];

    let changelog = generate_changelog(&commits, "0.1.0", None, None);
    assert!(!changelog.contains("https://"));
    assert!(changelog.contains("## [0.1.0]"));
}

#[test]
fn changelog_skips_release_chore_commits() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: real feature".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "chore: bump version to v0.1.0".into(),
        },
    ];

    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), None);
    assert!(changelog.contains("real feature"));
    assert!(!changelog.contains("bump version"));
}

#[test]
fn changelog_change_type() {
    let commits = vec![Commit {
        hash: "abc1234".into(),
        message: "change: rename config field".into(),
    }];

    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), None);
    assert!(changelog.contains("### Changed"));
    assert!(changelog.contains("rename config field"));
}

#[test]
fn changelog_empty_when_no_relevant_commits() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "chore: bump deps".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "ci: update workflow".into(),
        },
    ];

    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), None);
    // Should still have the version header but no sections
    assert!(changelog.contains("## [0.2.0]"));
    assert!(!changelog.contains("### "));
}

// Suppress unused import warning — ConventionalCommit is part of the public API
// and is used by the parse tests implicitly through type inference.
fn _assert_conventional_commit_is_public(_: ConventionalCommit) {}
