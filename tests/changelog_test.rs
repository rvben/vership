use vership::changelog::{
    ConventionalCommit, extract_section, generate_changelog, generate_changelog_with_mode,
    integrate_changelog, parse_conventional_commit,
};
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
    assert!(
        changelog.contains("[abc1234](https://github.com/rvben/vership/commit/abc1234def5678)")
    );
}

#[test]
fn changelog_includes_compare_link() {
    let commits = vec![Commit {
        hash: "abc1234".into(),
        message: "feat: add feature".into(),
    }];

    let base_url = "https://github.com/rvben/vership";
    let changelog = generate_changelog(&commits, "0.2.0", Some("0.1.0"), Some(base_url));
    assert!(changelog.contains("(https://github.com/rvben/vership/compare/v0.1.0...v0.2.0)"));
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

#[test]
fn strict_mode_errors_on_non_conventional_commit() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: proper commit".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "Update readme without conventional prefix".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "strict");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("non-conventional commit found"));
    assert!(err.contains("def5678"));
}

#[test]
fn strict_mode_succeeds_when_all_commits_conventional() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: add feature".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "fix: correct bug".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "strict");
    assert!(result.is_ok());
    let changelog = result.unwrap();
    assert!(changelog.contains("### Added"));
    assert!(changelog.contains("### Fixed"));
}

#[test]
fn strict_mode_skips_merge_commits() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: real change".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "Merge branch 'feature' into main".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "strict");
    assert!(result.is_ok());
}

#[test]
fn include_mode_adds_other_section_for_non_conventional() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: proper commit".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "Update readme without conventional prefix".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "include");
    assert!(result.is_ok());
    let changelog = result.unwrap();
    assert!(changelog.contains("### Added"));
    assert!(changelog.contains("### Other"));
    assert!(changelog.contains("Update readme without conventional prefix"));
}

#[test]
fn include_mode_omits_other_section_when_all_conventional() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: add export".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "fix: null check".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "include");
    assert!(result.is_ok());
    let changelog = result.unwrap();
    assert!(!changelog.contains("### Other"));
}

#[test]
fn include_mode_skips_merge_commits_in_other_section() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: real change".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "Merge branch 'feature' into main".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "include");
    assert!(result.is_ok());
    let changelog = result.unwrap();
    assert!(!changelog.contains("### Other"));
    assert!(!changelog.contains("Merge branch"));
}

#[test]
fn exclude_mode_silently_skips_non_conventional() {
    let commits = vec![
        Commit {
            hash: "abc1234".into(),
            message: "feat: add feature".into(),
        },
        Commit {
            hash: "def5678".into(),
            message: "This is not conventional".into(),
        },
    ];

    let result = generate_changelog_with_mode(&commits, "1.0.0", None, None, "exclude");
    assert!(result.is_ok());
    let changelog = result.unwrap();
    assert!(changelog.contains("### Added"));
    assert!(!changelog.contains("### Other"));
    assert!(!changelog.contains("This is not conventional"));
}

// --- integrate_changelog: [Unreleased] promotion ---

#[test]
fn integrate_no_existing_file_creates_header() {
    let section = "## [1.0.0] - 2026-05-22\n\n### Added\n\n- thing\n";
    let result = integrate_changelog(None, section);
    assert!(result.content.starts_with("# Changelog"));
    assert!(result.content.contains("## [1.0.0] - 2026-05-22"));
    assert!(
        !result.promoted,
        "no existing file means nothing to promote"
    );
}

#[test]
fn integrate_without_unreleased_prepends_above_latest() {
    let existing = "# Changelog\n\n## [0.9.0] - 2026-01-01\n\n### Added\n\n- old\n";
    let section = "## [1.0.0] - 2026-05-22\n\n### Added\n\n- new\n";
    let result = integrate_changelog(Some(existing), section);

    let new_pos = result.content.find("## [1.0.0]").unwrap();
    let old_pos = result.content.find("## [0.9.0]").unwrap();
    assert!(
        new_pos < old_pos,
        "new release must sit above the prior one"
    );
    assert!(!result.content.contains("## [Unreleased]"));
    assert!(
        !result.promoted,
        "no [Unreleased] section means legacy prepend, not promotion"
    );
}

#[test]
fn integrate_promotes_curated_unreleased_content() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- hand written fix\n\n## [0.1.5] - 2026-05-01\n\n### Added\n\n- prior\n";
    // Generated section body is intentionally different to prove curated content wins.
    let section = "## [0.1.6](https://example.com/compare/v0.1.5...v0.1.6) - 2026-05-22\n\n### Added\n\n- generated entry that should NOT appear\n";
    let result = integrate_changelog(Some(existing), section);

    // A fresh empty [Unreleased] sits at the top.
    let unreleased_pos = result.content.find("## [Unreleased]").unwrap();
    let release_pos = result.content.find("## [0.1.6]").unwrap();
    let prior_pos = result.content.find("## [0.1.5]").unwrap();
    assert!(unreleased_pos < release_pos);
    assert!(release_pos < prior_pos);

    // The promoted section keeps the curated content and the generated header (with link).
    assert!(result.content.contains("- hand written fix"));
    assert!(
        result
            .content
            .contains("## [0.1.6](https://example.com/compare/v0.1.5...v0.1.6) - 2026-05-22")
    );
    // Curated content wins: the generated body is discarded.
    assert!(
        !result
            .content
            .contains("generated entry that should NOT appear")
    );
    assert!(result.promoted, "curated content was promoted");
}

#[test]
fn integrate_empty_unreleased_falls_back_to_generated_body() {
    let existing =
        "# Changelog\n\n## [Unreleased]\n\n## [0.1.5] - 2026-05-01\n\n### Added\n\n- prior\n";
    let section = "## [0.1.6] - 2026-05-22\n\n### Fixed\n\n- generated fix\n";
    let result = integrate_changelog(Some(existing), section);

    let unreleased_pos = result.content.find("## [Unreleased]").unwrap();
    let release_pos = result.content.find("## [0.1.6]").unwrap();
    let prior_pos = result.content.find("## [0.1.5]").unwrap();
    assert!(unreleased_pos < release_pos);
    assert!(release_pos < prior_pos);
    // With nothing curated, the generated body fills the new release.
    assert!(result.content.contains("- generated fix"));
    assert!(
        !result.promoted,
        "an empty [Unreleased] is filled from generation, not promoted"
    );
}

#[test]
fn extract_section_returns_promoted_release_for_preview() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- hand written fix\n\n## [0.1.5] - 2026-05-01\n\n### Added\n\n- prior\n";
    let section = "## [0.1.6] - 2026-05-22\n\n### Added\n\n- generated\n";
    let full = integrate_changelog(Some(existing), section);

    let preview = extract_section(&full.content, "0.1.6").unwrap();
    assert!(preview.starts_with("## [0.1.6] - 2026-05-22"));
    assert!(preview.contains("- hand written fix"));
    // Must stop before the next release heading.
    assert!(!preview.contains("## [0.1.5]"));
    assert!(!preview.contains("## [Unreleased]"));
}

#[test]
fn integrate_strips_stale_version_link_refs_on_promotion() {
    // A Keep-a-Changelog file carrying bottom link-reference definitions:
    // the [Unreleased] ref points at the now-stale compare range, the prior
    // version has its own ref, and a non-version ref (CONTRIBUTING) coexists.
    let existing = "# Changelog\n\
        \n\
        ## [Unreleased]\n\
        \n\
        ### Fixed\n\
        \n\
        - curated fix\n\
        \n\
        ## [0.1.5] - 2026-05-01\n\
        \n\
        ### Added\n\
        \n\
        - prior\n\
        \n\
        [Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD\n\
        [0.1.5]: https://github.com/o/r/releases/tag/v0.1.5\n\
        [contributing]: https://github.com/o/r/blob/main/CONTRIBUTING.md\n";
    let section = "## [0.1.6](https://github.com/o/r/compare/v0.1.5...v0.1.6) - 2026-05-22\n\n### Added\n\n- gen\n";
    let result = integrate_changelog(Some(existing), section);

    // Promotion happened as before.
    assert!(result.promoted);
    assert!(result.content.contains("- curated fix"));
    assert!(
        result
            .content
            .contains("## [0.1.6](https://github.com/o/r/compare/v0.1.5...v0.1.6) - 2026-05-22")
    );

    // The stale version link-reference definitions are gone: the inline-linked
    // headers are self-contained, so these would only drift out of date.
    assert!(
        !result
            .content
            .contains("[Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD"),
        "stale [Unreleased] link-ref must be stripped"
    );
    assert!(
        !result
            .content
            .contains("[0.1.5]: https://github.com/o/r/releases/tag/v0.1.5"),
        "version link-ref definition must be stripped"
    );

    // Non-version link-reference definitions are preserved untouched.
    assert!(
        result
            .content
            .contains("[contributing]: https://github.com/o/r/blob/main/CONTRIBUTING.md"),
        "non-version link-ref must be preserved"
    );

    // The [Unreleased] *heading* (not the link-ref) still leads the document.
    assert_eq!(result.content.matches("## [Unreleased]").count(), 1);
    // No trailing blank-line cruft left where the ref block used to be.
    assert!(result.content.ends_with("CONTRIBUTING.md\n"));
    assert!(!result.content.ends_with("\n\n"));
}

#[test]
fn integrate_preserves_numeric_footnote_link_refs() {
    // Numeric reference definitions like `[1]:` / `[123]:` are footnote or
    // issue links, NOT version refs. A version label always has a major.minor
    // shape, so a bare integer label must be preserved on promotion.
    let existing = "# Changelog\n\
        \n\
        ## [Unreleased]\n\
        \n\
        ### Fixed\n\
        \n\
        - fix referencing a footnote[^1]\n\
        \n\
        ## [0.1.5] - 2026-05-01\n\
        \n\
        ### Added\n\
        \n\
        - prior\n\
        \n\
        [Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD\n\
        [0.1.5]: https://github.com/o/r/releases/tag/v0.1.5\n\
        [1]: https://github.com/o/r/issues/1\n\
        [123]: https://example.com/footnote\n";
    let section = "## [0.1.6](https://github.com/o/r/compare/v0.1.5...v0.1.6) - 2026-05-22\n\n### Added\n\n- gen\n";
    let result = integrate_changelog(Some(existing), section);

    // Version refs are stripped.
    assert!(
        !result
            .content
            .contains("[Unreleased]: https://github.com/o/r/compare/v0.1.5...HEAD")
    );
    assert!(
        !result
            .content
            .contains("[0.1.5]: https://github.com/o/r/releases/tag/v0.1.5")
    );

    // Numeric footnote/issue refs are not versions and must survive.
    assert!(
        result
            .content
            .contains("[1]: https://github.com/o/r/issues/1"),
        "numeric footnote ref [1] must be preserved, got:\n{}",
        result.content
    );
    assert!(
        result
            .content
            .contains("[123]: https://example.com/footnote"),
        "numeric footnote ref [123] must be preserved, got:\n{}",
        result.content
    );
}

#[test]
fn integrate_promotion_keeps_single_unreleased_heading() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- curated change\n";
    let section = "## [2.0.0] - 2026-05-22\n\n### Changed\n\n- gen\n";
    let result = integrate_changelog(Some(existing), section);

    assert_eq!(result.content.matches("## [Unreleased]").count(), 1);
    assert!(result.content.contains("- curated change"));
    assert!(result.content.contains("## [2.0.0] - 2026-05-22"));
    assert!(result.promoted, "curated change was promoted");
}
