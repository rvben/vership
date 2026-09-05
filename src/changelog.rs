use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::Local;
use regex::{Captures, Regex};

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
    let normalized = message.replace("\r\n", "\n");
    let has_breaking_footer = normalized.rsplit_once("\n\n").is_some_and(|(_, footer)| {
        footer.lines().any(|line| {
            line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
        })
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

#[derive(Clone, Copy)]
struct MarkdownFence {
    marker: char,
    length: usize,
}

fn fence_run(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then(|| (marker, length, &rest[length..]))
}

/// Update fence state and return true when this line is part of a fence.
fn scan_fence_line(line: &str, fence: &mut Option<MarkdownFence>) -> bool {
    if let Some(open) = *fence {
        if let Some((marker, length, suffix)) = fence_run(line)
            && marker == open.marker
            && length >= open.length
            && suffix.trim().is_empty()
        {
            *fence = None;
        }
        return true;
    }
    if let Some((marker, length, suffix)) = fence_run(line) {
        // A backtick info string cannot contain a backtick. Treating such a
        // line as prose matches CommonMark and prevents false fence state.
        if marker != '`' || !suffix.contains('`') {
            *fence = Some(MarkdownFence { marker, length });
            return true;
        }
    }
    false
}

fn find_line_outside_fences(
    content: &str,
    from: usize,
    mut predicate: impl FnMut(&str) -> bool,
) -> Option<usize> {
    let mut fence = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let line_without_ending = line.trim_end_matches(['\n', '\r']);
        if !scan_fence_line(line_without_ending, &mut fence)
            && offset >= from
            && predicate(line_without_ending)
        {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn find_h2_outside_fences(content: &str, from: usize) -> Option<usize> {
    find_line_outside_fences(content, from, |line| {
        line.starts_with("## ") || line.starts_with("##\t")
    })
}

fn prepend_to_changelog_checked(existing: Option<&str>, new_section: &str) -> String {
    let header = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/).\n";
    match existing {
        Some(content) => match find_h2_outside_fences(content, 0) {
            Some(position) => {
                let (preamble, releases) = content.split_at(position);
                join_blocks(&[preamble, new_section, releases])
            }
            None => join_blocks(&[content, new_section]),
        },
        None => join_blocks(&[header, new_section]),
    }
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
}

/// How curated `## [Unreleased]` notes combine with the entries generated from
/// commits when a release is promoted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CuratedPolicy {
    /// Keep the curated notes and add every generated entry they do not cover.
    /// A note covers a commit by citing its hash, so a hand-written entry that
    /// names its commit stands in for the generated one.
    #[default]
    Merge,
    /// The curated notes are the whole release: every generated entry is
    /// dropped and reported.
    Replace,
}

impl CuratedPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "merge" => Some(Self::Merge),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Replace => "replace",
        }
    }
}

/// Detailed result returned by the checked integration path.
#[derive(Debug)]
pub struct ChangelogIntegration {
    pub content: String,
    pub promoted: bool,
    /// Number of generated commit entries left out because curated notes are
    /// authoritative for them. Callers surface this so replacement is never
    /// silent. Equal to `omitted_entries.len()`.
    pub replaced_generated_entries: usize,
    /// Generated entries added alongside the curated notes, in generated
    /// order, each without its leading `- `.
    pub merged_entries: Vec<String>,
    /// Generated entries left out: every entry under the replace policy, and
    /// the entries whose commit the curated notes cite under merge.
    pub omitted_entries: Vec<String>,
}

/// Merge a generated release section into an existing CHANGELOG.md using the
/// default [`CuratedPolicy::Merge`]. See [`integrate_changelog_with_policy`].
pub fn integrate_changelog_checked(
    existing: Option<&str>,
    new_section: &str,
) -> std::result::Result<ChangelogIntegration, String> {
    integrate_changelog_with_policy(existing, new_section, CuratedPolicy::default())
}

/// Merge a generated release section into an existing CHANGELOG.md.
///
/// When the file has a canonical `## [Unreleased]` heading or the common
/// unbracketed `## Unreleased` form, that section is *promoted* into the release:
/// its heading becomes the new version's heading and a fresh empty
/// `## [Unreleased]` is inserted at the top. An empty section takes the
/// generated body. Hand-curated notes combine with the generated entries
/// according to `policy`: merged by default, so a note written for what the
/// commits cannot say never costs the release its other entries.
///
/// When there is no `## [Unreleased]` section, the generated section is simply
/// prepended above the most recent release (legacy behaviour).
pub fn integrate_changelog_with_policy(
    existing: Option<&str>,
    new_section: &str,
    policy: CuratedPolicy,
) -> std::result::Result<ChangelogIntegration, String> {
    let Some(content) = existing else {
        return Ok(ChangelogIntegration {
            content: prepend_to_changelog_checked(None, new_section),
            promoted: false,
            replaced_generated_entries: 0,
            merged_entries: Vec::new(),
            omitted_entries: Vec::new(),
        });
    };

    // Accept both canonical Keep a Changelog headings and the widespread
    // unbracketed form. Reject near-matches and duplicates instead of silently
    // publishing a release that strands hand-written notes.
    let unreleased_re =
        Regex::new(r"(?im)^##[ \t]+(?:\[unreleased\](?:\([^)]+\))?|unreleased)[ \t]*\r?$")
            .expect("valid regex");
    let candidates: Vec<_> = Regex::new(r"(?im)^##[^\n]*unreleased[^\n]*$")
        .expect("valid regex")
        .find_iter(content)
        .filter(|candidate| !is_in_markdown_fence(content, candidate.start()))
        .collect();
    let matches: Vec<_> = unreleased_re
        .find_iter(content)
        .filter(|candidate| !is_in_markdown_fence(content, candidate.start()))
        .collect();
    if candidates.len() > 1 {
        return Err(
            "multiple Unreleased-like headings found; keep exactly one supported heading before releasing"
                .into(),
        );
    }
    let Some(m) = matches.first().copied() else {
        if !candidates.is_empty() {
            return Err(
                "unsupported Unreleased heading; use `## [Unreleased]` or `## Unreleased`".into(),
            );
        }
        return Ok(ChangelogIntegration {
            content: prepend_to_changelog_checked(Some(content), new_section),
            promoted: false,
            replaced_generated_entries: 0,
            merged_entries: Vec::new(),
            omitted_entries: Vec::new(),
        });
    };

    let preamble = &content[..m.start()];
    // Headings shown inside fenced examples are content, not section bounds.
    let next_heading = find_h2_outside_fences(content, m.end()).unwrap_or(content.len());
    let unreleased_body = &content[m.end()..next_heading];
    let rest = &content[next_heading..];

    // Split the generated section into its heading line and body.
    let new_header = new_section.lines().next().unwrap_or(new_section);
    let generated_body = new_section.strip_prefix(new_header).unwrap_or("");

    let curated = !unreleased_body.trim().is_empty();
    let (promoted_section, merged_entries, omitted_entries) = if !curated {
        // Nothing curated: fill the promoted slot with the generated section.
        (new_section.to_string(), Vec::new(), Vec::new())
    } else {
        match policy {
            // Curated content is the whole release; reuse only the generated heading.
            CuratedPolicy::Replace => (
                join_blocks(&[new_header, unreleased_body]),
                Vec::new(),
                generated_entries(generated_body),
            ),
            CuratedPolicy::Merge => {
                let merge = merge_generated_entries(unreleased_body, generated_body);
                (
                    join_blocks(&[new_header, &merge.body]),
                    merge.merged,
                    merge.omitted,
                )
            }
        }
    };

    let result = join_blocks(&[preamble, "## [Unreleased]", &promoted_section, rest]);
    // vership emits self-contained inline-linked version headers, so any bottom
    // `[Unreleased]:` / `[x.y.z]:` link-reference definitions would only drift
    // out of date on every promotion. Every reference that used one is
    // rewritten to an inline link first, then the definitions are stripped,
    // leaving a clean trailing newline. Non-version definitions are preserved.
    let result = inline_version_link_refs(&result);
    Ok(ChangelogIntegration {
        content: result,
        promoted: curated,
        replaced_generated_entries: omitted_entries.len(),
        merged_entries,
        omitted_entries,
    })
}

/// A `### ` section of a generated release body, or the entries that precede
/// any heading.
struct GeneratedSection {
    name: Option<String>,
    entries: Vec<String>,
}

/// Parse the body of a generated release section (everything below its version
/// heading) into its `### ` sections and their `- ` entries. Entries are stored
/// without the list marker; an indented continuation line stays with its entry.
fn parse_generated_sections(body: &str) -> Vec<GeneratedSection> {
    let mut sections: Vec<GeneratedSection> = Vec::new();
    for line in body.lines() {
        if let Some(name) = h3_name(line) {
            sections.push(GeneratedSection {
                name: Some(name),
                entries: Vec::new(),
            });
        } else if let Some(entry) = line.strip_prefix("- ") {
            if sections.is_empty() {
                sections.push(GeneratedSection {
                    name: None,
                    entries: Vec::new(),
                });
            }
            sections
                .last_mut()
                .expect("a section was just pushed")
                .entries
                .push(entry.to_string());
        } else if !line.trim().is_empty()
            && let Some(section) = sections.last_mut()
            && let Some(entry) = section.entries.last_mut()
        {
            entry.push('\n');
            entry.push_str(line);
        }
    }
    sections
}

/// Every `- ` entry of a generated release body, in order, without markers.
fn generated_entries(body: &str) -> Vec<String> {
    parse_generated_sections(body)
        .into_iter()
        .flat_map(|section| section.entries)
        .collect()
}

/// The text of an ATX level-3 heading line, without its marker and any closing
/// `#` run. `None` for any other line, including deeper headings.
fn h3_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("###")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let name = rest.trim().trim_end_matches('#').trim_end();
    (!name.is_empty()).then(|| name.to_string())
}

fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    let digits = trimmed.trim_start_matches(|character: char| character.is_ascii_digit());
    digits.len() < trimmed.len() && (digits.starts_with(". ") || digits.starts_with(") "))
}

/// Hex tokens of commit-hash length in the curated notes, outside fenced code.
/// A generated entry whose commit starts with one of them is already covered.
fn cited_commit_hashes(curated: &str) -> Vec<String> {
    static HEX_TOKEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{7,40}\b").expect("valid regex"));
    let mut fence = None;
    let mut tokens = Vec::new();
    for line in curated.lines() {
        if scan_fence_line(line, &mut fence) {
            continue;
        }
        tokens.extend(
            HEX_TOKEN
                .find_iter(line)
                .map(|token| token.as_str().to_ascii_lowercase()),
        );
    }
    tokens
}

/// The commit hash a generated entry links to, lowercase. The full hash from
/// the commit URL when there is one, else the short hash of the link text.
fn entry_commit_hash(entry: &str) -> Option<String> {
    static COMMIT_URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/commit/([0-9a-fA-F]{7,40})").expect("valid regex"));
    static SHORT_LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\(\[([0-9a-fA-F]{7,40})\]\(").expect("valid regex"));
    COMMIT_URL
        .captures(entry)
        .or_else(|| SHORT_LINK.captures(entry))
        .map(|captures| captures[1].to_ascii_lowercase())
}

fn is_cited(entry: &str, cited: &[String]) -> bool {
    entry_commit_hash(entry).is_some_and(|hash| {
        cited
            .iter()
            .any(|token| hash.starts_with(token.as_str()) || token.starts_with(&hash))
    })
}

/// Render a generated entry for a terminal report: the trailing
/// `([abc1234](https://.../commit/...))` link becomes `(abc1234)`.
pub fn entry_summary(entry: &str) -> String {
    static COMMIT_LINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\s*\(\[([0-9a-fA-F]{7,40})\]\([^)]*\)\)\s*$").expect("valid regex")
    });
    COMMIT_LINK.replace(entry, " ($1)").into_owned()
}

struct MergeOutcome {
    body: String,
    merged: Vec<String>,
    omitted: Vec<String>,
}

/// Add the generated entries the curated notes do not cover. Each generated
/// `### ` section lands at the end of the curated section of the same name,
/// tight against a closing list or after a blank line otherwise; a section the
/// notes lack is appended in generated order; entries under no heading go
/// before the first curated heading. Curated text is never reordered.
fn merge_generated_entries(curated: &str, generated: &str) -> MergeOutcome {
    let cited = cited_commit_hashes(curated);
    // The blank lines around the body belong to the heading that carried it;
    // `join_blocks` restores exactly one on each side.
    let mut lines: Vec<String> = curated
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .map(str::to_string)
        .collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let mut fence = None;
    let mut headings: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if scan_fence_line(line, &mut fence) {
            continue;
        }
        if let Some(name) = h3_name(line) {
            headings.push((index, name));
        }
    }

    let mut merged = Vec::new();
    let mut omitted = Vec::new();
    let mut insertions: Vec<(usize, Vec<String>)> = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    for section in parse_generated_sections(generated) {
        let mut entries = Vec::new();
        for entry in section.entries {
            if is_cited(&entry, &cited) {
                omitted.push(entry);
            } else {
                entries.push(entry);
            }
        }
        if entries.is_empty() {
            continue;
        }
        let bullets = entries.iter().map(|entry| format!("- {entry}"));

        // The span of curated lines this section extends, if any.
        let target = match &section.name {
            Some(name) => headings
                .iter()
                .position(|(_, heading)| heading.eq_ignore_ascii_case(name))
                .map(|position| {
                    let start = headings[position].0 + 1;
                    let end = headings
                        .get(position + 1)
                        .map_or(lines.len(), |(index, _)| *index);
                    (start, end)
                }),
            None => Some((0, headings.first().map_or(lines.len(), |(index, _)| *index))),
        };
        match target {
            Some((start, end)) => {
                let last_content = (start..end)
                    .rev()
                    .find(|index| !lines[*index].trim().is_empty());
                let mut block = Vec::new();
                let at = match last_content {
                    Some(index) => {
                        if !is_list_item(&lines[index]) {
                            block.push(String::new());
                        }
                        index + 1
                    }
                    None => {
                        if start > 0 {
                            block.push(String::new());
                        }
                        start
                    }
                };
                block.extend(bullets);
                if at < lines.len() && !lines[at].trim().is_empty() {
                    block.push(String::new());
                }
                insertions.push((at, block));
            }
            None => {
                if !lines.is_empty() || !tail.is_empty() {
                    tail.push(String::new());
                }
                tail.push(format!(
                    "### {}",
                    section.name.as_deref().unwrap_or_default()
                ));
                tail.push(String::new());
                tail.extend(bullets);
            }
        }
        merged.extend(entries);
    }

    // Splice from the bottom up so earlier insertion points stay valid.
    insertions.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, block) in insertions {
        lines.splice(at..at, block);
    }
    lines.extend(tail);
    MergeOutcome {
        body: lines.join("\n"),
        merged,
        omitted,
    }
}

fn is_in_markdown_fence(content: &str, offset: usize) -> bool {
    let mut fence = None;
    for line in content[..offset].lines() {
        scan_fence_line(line, &mut fence);
    }
    fence.is_some()
}

/// Compatibility wrapper preserving the permissive 0.5.x behavior.
/// Release paths use [`integrate_changelog_checked`] so ambiguity is returned
/// as an actionable error rather than silently ignored, while library callers
/// upgrading within the patch line never gain a new panic path.
pub fn integrate_changelog(existing: Option<&str>, new_section: &str) -> ChangelogUpdate {
    let Some(content) = existing else {
        return ChangelogUpdate {
            content: prepend_to_changelog(None, new_section),
            promoted: false,
        };
    };
    let unreleased_re = Regex::new(r"(?m)^## \[Unreleased\][^\n]*$").expect("valid regex");
    let Some(m) = unreleased_re.find(content) else {
        return ChangelogUpdate {
            content: prepend_to_changelog(Some(content), new_section),
            promoted: false,
        };
    };
    let preamble = &content[..m.start()];
    let after_heading = &content[m.end()..];
    let next_heading = Regex::new(r"(?m)^## ").expect("valid regex");
    let (unreleased_body, rest) = match next_heading.find(after_heading) {
        Some(heading) => (
            &after_heading[..heading.start()],
            &after_heading[heading.start()..],
        ),
        None => (after_heading, ""),
    };
    let new_header = new_section.lines().next().unwrap_or(new_section);
    let promoted = !unreleased_body.trim().is_empty();
    let promoted_section = if promoted {
        join_blocks(&[new_header, unreleased_body])
    } else {
        new_section.to_string()
    };
    let content = inline_version_link_refs(&join_blocks(&[
        preamble,
        "## [Unreleased]",
        &promoted_section,
        rest,
    ]));
    ChangelogUpdate { content, promoted }
}

/// Remove changelog version link-reference definitions, the bottom
/// `[Unreleased]: <url>` and `[x.y.z]: <url>` lines, without breaking a link
/// that used them: every reference-style link to a version label (a
/// `## [1.2.0] - date` heading, `see [1.2.0]`, `[1.2.0][]`, `[text][1.2.0]`)
/// is rewritten to an inline link first, so the document renders exactly as it
/// did.
///
/// A version label is `Unreleased` or a `MAJOR.MINOR(.PATCH)` number with an
/// optional `v` prefix and optional pre-release/build suffix. Labels without
/// that shape are kept, so prose references such as `[contributing]:` and
/// numeric footnote/issue refs such as `[1]:` survive. Definitions and
/// references inside fenced code are content and stay as written. The
/// `[Unreleased]` definition is only stripped: its compare range is stale by
/// construction and the heading is regenerated bare. Collapses any blank lines
/// left behind into a single trailing newline.
fn inline_version_link_refs(content: &str) -> String {
    static DEFINITION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^ {0,3}\[((?i:unreleased)|v?\d+\.\d+(?:\.\d+)?(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\]:\s+(<[^>]*>|\S+)",
        )
        .expect("valid regex")
    });
    let lines: Vec<&str> = content.lines().collect();
    let mut is_definition = vec![false; lines.len()];
    let mut definitions: Vec<(String, String)> = Vec::new();
    let mut fence = None;
    for (index, line) in lines.iter().enumerate() {
        if scan_fence_line(line, &mut fence) {
            continue;
        }
        let Some(captures) = DEFINITION.captures(line) else {
            continue;
        };
        is_definition[index] = true;
        let label = &captures[1];
        // CommonMark resolves a label to its first definition.
        if label.eq_ignore_ascii_case("unreleased")
            || definitions
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(label))
        {
            continue;
        }
        let url = captures[2].trim_start_matches('<').trim_end_matches('>');
        definitions.push((label.to_string(), url.to_string()));
    }

    let mut fence = None;
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if scan_fence_line(line, &mut fence) {
            kept.push((*line).to_string());
        } else if !is_definition[index] {
            kept.push(inline_reference_links(line, &definitions));
        }
    }
    format!("{}\n", kept.join("\n").trim_end())
}

/// Rewrite the reference-style links on one line that resolve through
/// `definitions` into inline links. Code spans are left as written.
fn inline_reference_links(line: &str, definitions: &[(String, String)]) -> String {
    if definitions.is_empty() || !line.contains('[') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    for (segment, is_code) in code_span_segments(line) {
        if is_code {
            out.push_str(segment);
        } else {
            out.push_str(&rewrite_reference_links(segment, definitions));
        }
    }
    out
}

/// Split a line into text and code-span segments. A backtick run opens a code
/// span only when a run of exactly the same length closes it later on the
/// line; an unmatched run is text.
fn code_span_segments(line: &str) -> Vec<(&str, bool)> {
    let bytes = line.as_bytes();
    let mut segments = Vec::new();
    let mut text_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }
        let run_start = index;
        while index < bytes.len() && bytes[index] == b'`' {
            index += 1;
        }
        let run_length = index - run_start;
        let mut cursor = index;
        let mut close = None;
        while cursor < bytes.len() {
            if bytes[cursor] != b'`' {
                cursor += 1;
                continue;
            }
            let close_start = cursor;
            while cursor < bytes.len() && bytes[cursor] == b'`' {
                cursor += 1;
            }
            if cursor - close_start == run_length {
                close = Some(cursor);
                break;
            }
        }
        if let Some(close_end) = close {
            if run_start > text_start {
                segments.push((&line[text_start..run_start], false));
            }
            segments.push((&line[run_start..close_end], true));
            text_start = close_end;
            index = close_end;
        }
    }
    if text_start < bytes.len() {
        segments.push((&line[text_start..], false));
    }
    segments
}

fn rewrite_reference_links(text: &str, definitions: &[(String, String)]) -> String {
    // A bracketed span, then optionally a second bracket pair (full or
    // collapsed reference) or the `(` that marks an inline link.
    static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(!?)\[([^\[\]]+)\](\[([^\[\]]*)\]|\()?").expect("valid regex")
    });
    let lookup = |label: &str| {
        definitions
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(label))
            .map(|(_, url)| url.as_str())
    };
    REFERENCE
        .replace_all(text, |captures: &Captures| {
            let whole = captures[0].to_string();
            if !captures[1].is_empty() {
                return whole;
            }
            let link_text = &captures[2];
            let label = match captures.get(3).map(|m| m.as_str()) {
                Some("(") => return whole,
                Some(_) => match captures.get(4).map(|m| m.as_str()) {
                    Some(label) if !label.is_empty() => label,
                    _ => link_text,
                },
                None => link_text,
            };
            match lookup(label) {
                Some(url) => format!("[{link_text}]({url})"),
                None => whole,
            }
        })
        .into_owned()
}

/// Extract the changelog section for `version` from a full document: from its
/// `## [version]` heading up to (but not including) the next `## ` heading.
/// Useful for previewing exactly what a release section will contain.
pub fn extract_section<'a>(content: &'a str, version: &str) -> Option<&'a str> {
    let heading = version_heading_regex(version);
    let start = find_line_outside_fences(content, 0, |line| heading.is_match(line))?;
    let body_start = content[start..]
        .find('\n')
        .map(|index| start + index + 1)
        .unwrap_or(content.len());
    let end = find_h2_outside_fences(content, body_start).unwrap_or(content.len());
    Some(content[start..end].trim_end())
}

/// Check if a CHANGELOG.md already has an entry for the given version.
pub fn version_exists_in_changelog(content: &str, version: &str) -> bool {
    let heading = version_heading_regex(version);
    find_line_outside_fences(content, 0, |line| heading.is_match(line)).is_some()
}

fn version_heading_regex(version: &str) -> Regex {
    let escaped = regex::escape(version);
    Regex::new(&format!(
        r"(?m)^##[ \t]+\[{escaped}\](?:\([^\r\n]*\))?[ \t]*(?:-[ \t]*[^\r\n]+)?\r?$"
    ))
    .expect("escaped version produces a valid regex")
}
