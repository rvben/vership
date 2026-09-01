use std::collections::BTreeMap;

use chrono::Local;
use regex::Regex;

use crate::git::Commit;

#[derive(Debug, Clone)]
pub struct ConventionalCommit {
    pub commit_type: String,
    pub scope: Option<String>,
    pub description: String,
    pub breaking: bool,
}

/// Parse a conventional commit message. Returns None for non-conventional or merge commits.
pub fn parse_conventional_commit(message: &str) -> Option<ConventionalCommit> {
    let subject = message.lines().next()?;
    if subject.starts_with("Merge ") {
        return None;
    }

    let re = Regex::new(r"^(\w+)(?:\(([^)]+)\))?(!)?: (.+)$").expect("valid regex");
    let caps = re.captures(subject)?;
    let has_breaking_footer = message.lines().skip(1).any(|line| {
        let line = line.trim_start();
        line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
    });

    Some(ConventionalCommit {
        commit_type: caps[1].to_string(),
        scope: caps.get(2).map(|m| m.as_str().to_string()),
        breaking: caps.get(3).is_some() || has_breaking_footer,
        description: caps[4].to_string(),
    })
}

/// Map commit type to changelog section name. Returns None for excluded types.
fn type_to_section(commit_type: &str) -> Option<&'static str> {
    match commit_type {
        "feat" => Some("Added"),
        "fix" => Some("Fixed"),
        "perf" => Some("Performance"),
        "change" => Some("Changed"),
        _ => None,
    }
}

/// Generate a changelog section for a release.
///
/// - `commits`: list of commits since the previous tag
/// - `version`: new version string (without `v` prefix)
/// - `previous_tag`: previous tag string (with `v` prefix), or None for first release
/// - `remote_url`: base URL for commit/compare links (e.g. "https://github.com/rvben/vership")
pub fn generate_changelog(
    commits: &[Commit],
    version: &str,
    previous_tag: Option<&str>,
    remote_url: Option<&str>,
) -> String {
    let mut breaking: Vec<String> = Vec::new();
    let mut sections: BTreeMap<&str, Vec<String>> = BTreeMap::new();

    for commit in commits {
        let Some(cc) = parse_conventional_commit(&commit.message) else {
            continue;
        };

        let entry = format_entry(&cc, commit, remote_url);

        if cc.breaking {
            breaking.push(entry.clone());
        }

        // A breaking entry belongs in one place. Repeating it in its ordinary
        // type section makes generated release notes noisy and ambiguous.
        if !cc.breaking
            && let Some(section) = type_to_section(&cc.commit_type)
        {
            sections.entry(section).or_default().push(entry);
        }
    }

    let date = Local::now().format("%Y-%m-%d");
    let mut output = String::new();

    // Version header with optional compare link
    match (remote_url, previous_tag) {
        (Some(url), Some(prev)) => {
            // Normalize the previous tag to always have a `v` prefix
            let prev_tag = if prev.starts_with('v') {
                prev.to_string()
            } else {
                format!("v{prev}")
            };
            output.push_str(&format!(
                "## [{version}]({url}/compare/{prev_tag}...v{version}) - {date}\n",
            ));
        }
        _ => {
            output.push_str(&format!("## [{version}] - {date}\n"));
        }
    }

    // Section order: Breaking Changes first, then Added, Changed, Fixed, Performance
    let section_order = [
        "Breaking Changes",
        "Added",
        "Changed",
        "Fixed",
        "Performance",
    ];

    if !breaking.is_empty() {
        output.push_str("\n### Breaking Changes\n\n");
        for entry in &breaking {
            output.push_str(&format!("- {entry}\n"));
        }
    }

    for section_name in &section_order {
        if *section_name == "Breaking Changes" {
            continue; // Already handled above
        }
        if let Some(entries) = sections.get(section_name) {
            output.push_str(&format!("\n### {section_name}\n\n"));
            for entry in entries {
                output.push_str(&format!("- {entry}\n"));
            }
        }
    }

    output
}

fn format_entry(cc: &ConventionalCommit, commit: &Commit, remote_url: Option<&str>) -> String {
    let scope_prefix = cc
        .scope
        .as_ref()
        .map(|s| format!("**{s}**: "))
        .unwrap_or_default();

    let hash_suffix = match remote_url {
        Some(url) => {
            let short_hash = &commit.hash[..7.min(commit.hash.len())];
            format!(" ([{short_hash}]({url}/commit/{}))", commit.hash)
        }
        None => String::new(),
    };

    format!("{scope_prefix}{}{hash_suffix}", cc.description)
}

/// Generate changelog with unconventional commit handling mode.
///
/// - "exclude" (default): silently skip non-conventional commits
/// - "include": add them under an "Other" section
/// - "strict": return an error if any non-conventional commit is found
pub fn generate_changelog_with_mode(
    commits: &[Commit],
    version: &str,
    previous_tag: Option<&str>,
    remote_url: Option<&str>,
    unconventional_mode: &str,
) -> std::result::Result<String, String> {
    if unconventional_mode == "strict" {
        for commit in commits {
            if commit.message.starts_with("Merge ") {
                continue;
            }
            if parse_conventional_commit(&commit.message).is_none() {
                return Err(format!(
                    "non-conventional commit found (strict mode): {} {}",
                    &commit.hash[..7.min(commit.hash.len())],
                    commit.subject()
                ));
            }
        }
    }

    let mut output = generate_changelog(commits, version, previous_tag, remote_url);

    if unconventional_mode == "include" {
        let unconventional: Vec<&Commit> = commits
            .iter()
            .filter(|c| !c.message.starts_with("Merge "))
            .filter(|c| parse_conventional_commit(&c.message).is_none())
            .collect();

        if !unconventional.is_empty() {
            output.push_str("\n### Other\n\n");
            for commit in unconventional {
                let short_hash = &commit.hash[..7.min(commit.hash.len())];
                match remote_url {
                    Some(url) => output.push_str(&format!(
                        "- {} ([{short_hash}]({url}/commit/{}))\n",
                        commit.subject(),
                        commit.hash
                    )),
                    None => output.push_str(&format!("- {}\n", commit.subject())),
                }
            }
        }
    }

    Ok(output)
}

/// Prepend a new changelog section to an existing CHANGELOG.md content.
/// Creates the standard file structure when no existing content is provided.
pub fn prepend_to_changelog(existing: Option<&str>, new_section: &str) -> String {
    let header = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/).\n";

    match existing {
        Some(content) => match split_at_first_release_heading(content) {
            Some((preamble, releases)) => join_blocks(&[preamble, new_section, releases]),
            // No existing versions, append after the preamble.
            None => join_blocks(&[content, new_section]),
        },
        None => join_blocks(&[header, new_section]),
    }
}

/// Split a changelog at its first `## ` heading, giving the preamble before it
/// and the releases from it onwards. The preamble is empty when the document
/// opens with a heading. Returns `None` for a document with no `## ` heading.
fn split_at_first_release_heading(content: &str) -> Option<(&str, &str)> {
    let pos = if content.starts_with("## ") {
        0
    } else {
        content.find("\n## ")? + 1
    };
    Some(content.split_at(pos))
}

/// Join document blocks with exactly one blank line between them, ending in a
/// single newline. Blank-only blocks are dropped, so a run of blank lines never
/// grows at a join and a release never lands against an empty preamble.
fn join_blocks(blocks: &[&str]) -> String {
    let kept: Vec<&str> = blocks
        .iter()
        .map(|block| trim_blank_lines(block))
        .filter(|block| !block.is_empty())
        .collect();
    format!("{}\n", kept.join("\n\n"))
}

/// Drop whole blank lines from both ends of a block. The indentation of the
/// first content line survives, so a section opening with an indented code
/// block stays a code block.
fn trim_blank_lines(block: &str) -> &str {
    let mut bounds: Option<(usize, usize)> = None;
    let mut offset = 0;
    for line in block.split_inclusive('\n') {
        if !line.trim().is_empty() {
            let content_end = offset + line.trim_end_matches(['\n', '\r']).len();
            bounds = Some((bounds.map_or(offset, |(start, _)| start), content_end));
        }
        offset += line.len();
    }
    match bounds {
        Some((start, end)) => &block[start..end],
        None => "",
    }
}

/// Outcome of merging a generated release section into a CHANGELOG.md.
#[derive(Debug)]
pub struct ChangelogUpdate {
    /// The full updated changelog document.
    pub content: String,
    /// True when a curated `## [Unreleased]` section was promoted into the
    /// release, carrying its hand-written entries forward. False when the
    /// `[Unreleased]` section was empty (the generated body was used) or
    /// absent (legacy prepend).
    pub promoted: bool,
    /// Number of generated commit entries omitted because curated notes are
    /// authoritative. Callers surface this so replacement is never silent.
    pub replaced_generated_entries: usize,
}

/// Merge a generated release section into an existing CHANGELOG.md.
///
/// When the file has a canonical `## [Unreleased]` heading or the common
/// unbracketed `## Unreleased` form, that section is *promoted* into the release:
/// its heading becomes the new version's heading, a fresh empty
/// `## [Unreleased]` is inserted at the top, and any hand-curated entries are
/// preserved. The generated section is only used to supply the version heading
/// (and, when `[Unreleased]` is empty, its body).
///
/// When there is no `## [Unreleased]` section, the generated section is simply
/// prepended above the most recent release (legacy behaviour).
pub fn integrate_changelog_checked(
    existing: Option<&str>,
    new_section: &str,
) -> std::result::Result<ChangelogUpdate, String> {
    let Some(content) = existing else {
        return Ok(ChangelogUpdate {
            content: prepend_to_changelog(None, new_section),
            promoted: false,
            replaced_generated_entries: 0,
        });
    };

    // Accept both canonical Keep a Changelog headings and the widespread
    // unbracketed form. Reject near-matches and duplicates instead of silently
    // publishing a release that strands hand-written notes.
    let unreleased_re =
        Regex::new(r"(?im)^##[ \t]+(?:\[unreleased\](?:\([^)]+\))?|unreleased)[ \t]*\r?$")
            .expect("valid regex");
    let matches: Vec<_> = unreleased_re.find_iter(content).collect();
    if matches.len() > 1 {
        return Err("multiple Unreleased headings found; keep exactly one before releasing".into());
    }
    let Some(m) = matches.first().copied() else {
        let near_match = Regex::new(r"(?im)^##[^\n]*unreleased[^\n]*$")
            .expect("valid regex")
            .is_match(content);
        if near_match {
            return Err(
                "unsupported Unreleased heading; use `## [Unreleased]` or `## Unreleased`".into(),
            );
        }
        return Ok(ChangelogUpdate {
            content: prepend_to_changelog(Some(content), new_section),
            promoted: false,
            replaced_generated_entries: 0,
        });
    };

    let preamble = &content[..m.start()];
    // Everything after the [Unreleased] heading line.
    let after_heading = &content[m.end()..];

    // The [Unreleased] body runs until the next `## ` heading (or end of file).
    let next_heading = Regex::new(r"(?m)^##[ \t]+").expect("valid regex");
    let (unreleased_body, rest) = match next_heading.find(after_heading) {
        Some(h) => (&after_heading[..h.start()], &after_heading[h.start()..]),
        None => (after_heading, ""),
    };

    // Split the generated section into its heading line and body.
    let new_header = new_section.lines().next().unwrap_or(new_section);

    let curated = !unreleased_body.trim().is_empty();
    let replaced_generated_entries = if curated {
        new_section
            .lines()
            .filter(|line| line.trim_start().starts_with("- "))
            .count()
    } else {
        0
    };
    let promoted_section = if curated {
        // Curated content wins; reuse only the generated heading.
        join_blocks(&[new_header, unreleased_body])
    } else {
        // Nothing curated: fill the promoted slot with the generated section.
        new_section.to_string()
    };

    let result = join_blocks(&[preamble, "## [Unreleased]", &promoted_section, rest]);
    // vership emits self-contained inline-linked version headers, so any bottom
    // `[Unreleased]:` / `[x.y.z]:` link-reference definitions are redundant and
    // drift out of date on every promotion. Strip them, leaving a clean trailing
    // newline. Non-version link-reference definitions are preserved.
    let result = strip_version_link_refs(&result);
    Ok(ChangelogUpdate {
        content: result,
        promoted: curated,
        replaced_generated_entries,
    })
}

/// Compatibility wrapper for callers that only handle well-formed changelogs.
/// Release paths use [`integrate_changelog_checked`] so ambiguity is returned
/// as an actionable error rather than silently ignored.
#[deprecated(
    since = "0.5.21",
    note = "use integrate_changelog_checked to handle malformed headings"
)]
pub fn integrate_changelog(existing: Option<&str>, new_section: &str) -> ChangelogUpdate {
    integrate_changelog_checked(existing, new_section)
        .expect("ambiguous or malformed Unreleased heading")
}

/// Remove changelog version link-reference definitions: the bottom
/// `[Unreleased]: <url>` and `[x.y.z]: <url>` lines. A version label is
/// `Unreleased` or a `MAJOR.MINOR(.PATCH)` number with an optional `v` prefix
/// and optional pre-release/build suffix. Labels without that shape are kept,
/// so prose references such as `[contributing]:` and numeric footnote/issue
/// refs such as `[1]:` survive. Collapses any blank lines left behind into a
/// single trailing newline.
fn strip_version_link_refs(content: &str) -> String {
    let ref_re = Regex::new(
        r"^\[(?:Unreleased|v?\d+\.\d+(?:\.\d+)?(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\]:\s+\S",
    )
    .expect("valid regex");
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| !ref_re.is_match(line))
        .collect();
    format!("{}\n", kept.join("\n").trim_end())
}

/// Extract the changelog section for `version` from a full document: from its
/// `## [version]` heading up to (but not including) the next `## ` heading.
/// Useful for previewing exactly what a release section will contain.
pub fn extract_section<'a>(content: &'a str, version: &str) -> Option<&'a str> {
    let heading = format!("## [{version}]");
    let start = content.find(&heading)?;
    let rest = &content[start..];
    let body_start = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let next = Regex::new(r"(?m)^##[ \t]+")
        .expect("valid regex")
        .find(&rest[body_start..])
        .map(|m| body_start + m.start())
        .unwrap_or(rest.len());
    Some(rest[..next].trim_end())
}

/// Check if a CHANGELOG.md already has an entry for the given version.
pub fn version_exists_in_changelog(content: &str, version: &str) -> bool {
    content.contains(&format!("## [{version}]"))
}
