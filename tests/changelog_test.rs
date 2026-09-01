use vership::changelog::{
    ConventionalCommit, extract_section, generate_changelog, generate_changelog_with_mode,
    integrate_changelog_checked, parse_conventional_commit, version_exists_in_changelog,
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
fn parse_breaking_change_footer_from_full_commit_message() {
    let cc = parse_conventional_commit(
        "feat(api): replace token format\n\nBREAKING CHANGE: old tokens are no longer accepted",
    )
    .unwrap();
    assert!(cc.breaking);
    assert_eq!(cc.description, "replace token format");
}

#[test]
fn indented_breaking_change_example_is_not_a_footer() {
    let parsed = parse_conventional_commit(
        "docs: explain commit messages\n\nExample:\n    BREAKING CHANGE: describe impact",
    )
    .unwrap();
    assert!(!parsed.breaking);
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
    assert_eq!(
        changelog.matches("remove legacy API").count(),
        1,
        "a breaking feature must not be duplicated under Added"
    );
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

// --- integrate_changelog_checked: [Unreleased] promotion ---

#[test]
fn integrate_no_existing_file_creates_header() {
    let section = "## [1.0.0] - 2026-05-22\n\n### Added\n\n- thing\n";
    let result = integrate_changelog_checked(None, section).unwrap();
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
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

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
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

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
    assert_eq!(result.replaced_generated_entries, 1);
}

#[test]
fn integrate_promotes_unbracketed_unreleased_content() {
    let existing = "# Changelog\n\n## Unreleased\n\n### Added\n\n- hand written analytics\n\n## [0.1.5] - 2026-05-01\n";
    let section = "## [0.1.6] - 2026-05-22\n\n### Fixed\n\n- generated fix\n";

    let result = integrate_changelog_checked(Some(existing), section).unwrap();

    assert!(result.promoted);
    assert!(result.content.contains("## [0.1.6] - 2026-05-22"));
    assert!(result.content.contains("- hand written analytics"));
    assert!(!result.content.contains("- generated fix"));
    assert_eq!(result.content.matches("## [Unreleased]").count(), 1);
}

#[test]
fn integrate_handles_flexible_heading_whitespace_and_crlf() {
    let existing =
        "# Changelog\r\n\r\n##\tUnreleased\r\n\r\n- curated\r\n\r\n##   [0.9.0]\r\n\r\n- prior\r\n";
    let result =
        integrate_changelog_checked(Some(existing), "## [1.0.0] - 2026-05-22\n\n- generated\n")
            .unwrap();

    let release = extract_section(&result.content, "1.0.0").unwrap();
    assert!(release.contains("- curated"));
    assert!(!release.contains("##   [0.9.0]"));
    assert!(result.content.contains("##   [0.9.0]"));
}

#[test]
fn integrate_rejects_ambiguous_unreleased_headings() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n## Unreleased\n";
    let error = integrate_changelog_checked(Some(existing), "## [1.0.0] - 2026-05-22\n")
        .expect_err("duplicate release-note sources must fail closed");
    assert!(error.contains("multiple Unreleased-like headings"));
}

#[test]
fn integrate_rejects_valid_and_malformed_unreleased_headings_together() {
    let existing =
        "# Changelog\n\n## [Unreleased]\n\n- valid\n\n## Unreleased changes\n\n- stranded\n";
    let error = integrate_changelog_checked(Some(existing), "## [1.0.0] - 2026-05-22\n")
        .expect_err("every Unreleased-like heading must be classified");
    assert!(error.contains("multiple Unreleased-like headings"));
}

#[test]
fn integrate_rejects_malformed_unreleased_heading() {
    let existing = "# Changelog\n\n## [Unreleased notes]\n\n- important\n";
    let error = integrate_changelog_checked(Some(existing), "## [1.0.0] - 2026-05-22\n")
        .expect_err("a plausible but unsupported heading must not be ignored");
    assert!(error.contains("unsupported Unreleased heading"));
}

#[test]
fn checked_integration_ignores_unreleased_examples_in_fenced_code() {
    let existing = "# Changelog\n\n```md\n## [Unreleased]\n```\n\n## [0.9.0] - 2026-01-01\n";
    let section = "## [1.0.0] - 2026-09-01\n\n- shipped\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

    assert!(!result.promoted);
    let example = result.content.find("```md").unwrap();
    let release = result.content.find("## [1.0.0]").unwrap();
    let prior = result.content.find("## [0.9.0]").unwrap();
    assert!(example < release && release < prior);
    assert_eq!(result.content.matches("## [1.0.0]").count(), 1);
}

#[test]
fn fenced_h2_examples_do_not_end_curated_or_released_sections() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n- before\n\n```md\n## Example\n```\n\n- after\n\n## [0.9.0] - 2026-01-01\n";
    let section = "## [1.0.0] - 2026-09-01\n\n- generated\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();
    let released = extract_section(&result.content, "1.0.0").unwrap();

    assert!(released.contains("- before"));
    assert!(released.contains("## Example"));
    assert!(released.contains("- after"));
    assert!(!released.contains("## [0.9.0]"));
}

#[test]
fn shorter_fence_runs_do_not_close_a_longer_fence() {
    let existing =
        "# Changelog\n\n````md\n```\n## [Unreleased]\n```\n````\n\n## [0.9.0] - 2026-01-01\n";
    let section = "## [1.0.0] - 2026-09-01\n\n- shipped\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

    assert!(!result.promoted);
    let outer_fence = result.content.find("````md").unwrap();
    let release = result.content.find("## [1.0.0]").unwrap();
    let prior = result.content.find("## [0.9.0]").unwrap();
    assert!(outer_fence < release && release < prior);
}

#[test]
fn promotion_preserves_version_like_links_inside_fenced_examples() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n```md\n[1.2.3]: https://example.test/version\n[Unreleased]: https://example.test/unreleased\n```\n\n[1.2.3]: https://stale.test/version\n[Unreleased]: https://stale.test/unreleased\n";
    let section = "## [1.0.0] - 2026-09-01\n\n- generated\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();
    let released = extract_section(&result.content, "1.0.0").unwrap();

    assert!(released.contains("[1.2.3]: https://example.test/version"));
    assert!(released.contains("[Unreleased]: https://example.test/unreleased"));
    assert!(!result.content.contains("https://stale.test"));
}

#[test]
fn legacy_integration_remains_permissive_for_malformed_headings() {
    let existing = "# Changelog\n\n## [Unreleased notes]\n\n- legacy input\n";
    let result = vership::changelog::integrate_changelog(
        Some(existing),
        "## [1.0.0] - 2026-09-01\n\n- shipped\n",
    );

    assert!(!result.promoted);
    assert!(result.content.contains("## [1.0.0]"));
    assert!(result.content.contains("## [Unreleased notes]"));
}

#[test]
fn integrate_empty_unreleased_falls_back_to_generated_body() {
    let existing =
        "# Changelog\n\n## [Unreleased]\n\n## [0.1.5] - 2026-05-01\n\n### Added\n\n- prior\n";
    let section = "## [0.1.6] - 2026-05-22\n\n### Fixed\n\n- generated fix\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

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
    let full = integrate_changelog_checked(Some(existing), section).unwrap();

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
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

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
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

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
fn integrate_strips_full_semver_version_link_refs() {
    // A version with both a prerelease and build-metadata suffix is still a
    // version vership can release, so its bottom link-ref must be stripped too.
    let existing = "# Changelog\n\
        \n\
        ## [Unreleased]\n\
        \n\
        ### Fixed\n\
        \n\
        - curated\n\
        \n\
        ## [1.2.3-alpha.1+build.5] - 2026-05-01\n\
        \n\
        ### Added\n\
        \n\
        - prior\n\
        \n\
        [Unreleased]: https://github.com/o/r/compare/v1.2.3-alpha.1+build.5...HEAD\n\
        [1.2.3-alpha.1+build.5]: https://github.com/o/r/releases/tag/v1.2.3-alpha.1+build.5\n";
    let section = "## [1.2.4](https://github.com/o/r/compare/v1.2.3...v1.2.4) - 2026-05-22\n\n### Added\n\n- gen\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

    assert!(result.promoted);
    assert!(
        !result.content.contains(
            "[1.2.3-alpha.1+build.5]: https://github.com/o/r/releases/tag/v1.2.3-alpha.1+build.5"
        ),
        "full-semver version link-ref must be stripped, got:\n{}",
        result.content
    );
    assert!(
        !result
            .content
            .contains("[Unreleased]: https://github.com/o/r/compare/v1.2.3-alpha.1+build.5...HEAD"),
        "stale [Unreleased] ref must be stripped, got:\n{}",
        result.content
    );
}

#[test]
fn integrate_promotion_keeps_single_unreleased_heading() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- curated change\n";
    let section = "## [2.0.0] - 2026-05-22\n\n### Changed\n\n- gen\n";
    let result = integrate_changelog_checked(Some(existing), section).unwrap();

    assert_eq!(result.content.matches("## [Unreleased]").count(), 1);
    assert!(result.content.contains("- curated change"));
    assert!(result.content.contains("## [2.0.0] - 2026-05-22"));
    assert!(result.promoted, "curated change was promoted");
}

// --- integrate_changelog_checked: blank-line hygiene across repeated releases ---

const PREAMBLE: &str = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\nand this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n";

/// Longest run of consecutive blank lines anywhere in the document.
fn longest_blank_run(content: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

fn release_section(patch: u32) -> String {
    format!(
        "## [0.1.{patch}](https://example.com/compare/v0.1.{}...v0.1.{patch}) - 2026-06-0{patch}\n\n### Fixed\n\n- fix {patch}\n",
        patch - 1
    )
}

#[test]
fn repeated_prepends_do_not_accumulate_blank_lines() {
    let mut content = format!("{PREAMBLE}\n{}", release_section(1));
    for patch in 2..=5 {
        content = integrate_changelog_checked(Some(&content), &release_section(patch))
            .unwrap()
            .content;
    }

    assert_eq!(
        longest_blank_run(&content),
        1,
        "each release must be separated by exactly one blank line, got:\n{content}"
    );
    let newest = content.find("## [0.1.5]").unwrap();
    let oldest = content.find("## [0.1.1]").unwrap();
    assert!(newest < oldest, "newest release must sit on top");
}

#[test]
fn repeated_promotions_do_not_accumulate_blank_lines() {
    let mut content = format!("{PREAMBLE}\n## [Unreleased]\n\n{}", release_section(1));
    for patch in 2..=5 {
        content = integrate_changelog_checked(Some(&content), &release_section(patch))
            .unwrap()
            .content;
    }

    assert_eq!(
        longest_blank_run(&content),
        1,
        "each release must be separated by exactly one blank line, got:\n{content}"
    );
    assert_eq!(content.matches("## [Unreleased]").count(), 1);
}

#[test]
fn promoting_curated_content_leaves_one_blank_line_before_the_prior_release() {
    let existing = format!(
        "{PREAMBLE}\n## [Unreleased]\n\n### Added\n\n- curated\n\n{}",
        release_section(1)
    );
    let result = integrate_changelog_checked(Some(&existing), &release_section(2)).unwrap();

    assert!(result.promoted);
    assert!(
        result.content.contains("- curated\n\n## [0.1.1]"),
        "curated body must be one blank line above the prior release, got:\n{}",
        result.content
    );
}

#[test]
fn promoted_heading_is_separated_from_a_tight_curated_body() {
    let existing = "# Changelog\n\n## [Unreleased]\n### Added\n\n- curated\n";
    let result = integrate_changelog_checked(Some(existing), &release_section(1)).unwrap();

    assert!(
        result.content.contains("- 2026-06-01\n\n### Added"),
        "a body written tight against [Unreleased] still needs a blank line under the promoted heading, got:\n{}",
        result.content
    );
}

#[test]
fn promotion_keeps_the_indentation_of_a_curated_code_block() {
    let existing = "# Changelog\n\n## [Unreleased]\n\n    let x = 1;\n\n## [0.1.0] - 2026-01-01\n\n### Added\n\n- initial\n";
    let result = integrate_changelog_checked(Some(existing), &release_section(1)).unwrap();

    assert!(
        result.content.contains("\n    let x = 1;\n"),
        "an indented code block must survive promotion as a code block, got:\n{}",
        result.content
    );
}

#[test]
fn prepend_puts_the_new_release_above_a_changelog_that_opens_with_a_heading() {
    let existing = "## [0.1.1] - 2026-06-01\n\n### Fixed\n\n- fix 1\n";
    let result = integrate_changelog_checked(Some(existing), &release_section(2)).unwrap();

    let newest = result.content.find("## [0.1.2]").unwrap();
    let oldest = result.content.find("## [0.1.1]").unwrap();
    assert!(
        newest < oldest,
        "a changelog with no preamble still gets the new release on top, got:\n{}",
        result.content
    );
    assert!(
        !result.content.starts_with('\n'),
        "no leading blank line when there is no preamble, got:\n{}",
        result.content
    );
}

#[test]
fn version_detection_requires_an_exact_heading() {
    assert!(version_exists_in_changelog(
        "## [1.2.3] - 2026-09-01\n",
        "1.2.3"
    ));
    assert!(!version_exists_in_changelog(
        "```md\n## [1.2.3]\n```\n",
        "1.2.3"
    ));
    assert!(!version_exists_in_changelog(
        "## [1.2.30] - 2026-09-01\n",
        "1.2.3"
    ));
}
