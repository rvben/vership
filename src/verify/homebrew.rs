//! Read the stable package's static metadata without executing downloaded Ruby.
//!
//! Resource, bottle, head, and method bodies are not evidence of a package
//! version. Unknown or inconsistent metadata must fail verification rather
//! than accidentally confirm a version found elsewhere in the file.

use regex::Regex;
use std::sync::LazyLock;

static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(version|url)\s+["']([^"']+)["']"#).unwrap());
static DECLARATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:version|url)\b").unwrap());
static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bdo(?:\s*\|[^|]*\|)?$").unwrap());
static MODIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:if|unless|rescue|while|until)\b|;").unwrap());
static EXCLUDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:resource|bottle|head|patch|livecheck|test|def|service|caveats)(?:\s|\(|$)")
        .unwrap()
});
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:^|,)\s*tag:\s*["']([^"']+)["']"#).unwrap());
static TAG_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|,)\s*tag:").unwrap());
static HEREDOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^<<[-~]?["']?([A-Za-z_][A-Za-z_0-9]*)["']?"#).unwrap());
static VERSION_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:\.\d+)+").unwrap());
static VERSION_SEGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^v?(\d+(?:\.\d+)+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?)$").unwrap()
});
static FILE_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    // Arbitrary semver prerelease labels are ambiguous with binary target
    // suffixes. Infer common source prereleases; other layouts need a tag or
    // an explicit version rather than guessing that `linux-arm64` is a version.
    Regex::new(r"(?:^|[-_])v?(\d+(?:\.\d+)+(?:-(?:(?:alpha|beta|rc|pre|dev)[0-9A-Za-z.-]*|\d+(?:\.\d+)*))?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?)$").unwrap()
});

pub(super) fn formula_version(body: &str) -> Result<String, String> {
    let mut skip_indent = None;
    let mut comment = false;
    let mut versions: Vec<(Vec<usize>, String)> = Vec::new();
    let mut urls: Vec<(Vec<usize>, Option<String>)> = Vec::new();
    let mut scopes: Vec<(usize, usize)> = Vec::new();
    let mut next_scope = 0;
    let mut pending_url: Option<(usize, usize)> = None;
    let mut heredoc: Option<(String, bool)> = None;
    for line in body.lines() {
        if let Some((marker, indented)) = &heredoc {
            if (if *indented { line.trim() } else { line }) == marker {
                heredoc = None;
            }
            continue;
        }
        if line == "__END__" {
            break;
        }
        if line.starts_with("=begin") {
            comment = true;
        }
        if line.starts_with("=end") {
            comment = false;
            continue;
        }
        if comment || line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let line = strip_comment(line.trim()).trim_end();
        // Track heredocs even inside excluded method/test bodies: their text
        // can be flush-left and must not escape the indentation-based skip.
        if let Some(marker) = heredoc_marker(line)? {
            if DECLARATION.is_match(line) && skip_indent.is_none_or(|start| indent <= start) {
                return Err("heredoc version or source URL declarations are unsupported".into());
            }
            heredoc = Some(marker);
            continue;
        }
        if let Some(start) = skip_indent {
            if indent > start {
                continue;
            }
            skip_indent = None;
        }
        if let Some((start, index)) = pending_url {
            if indent > start {
                if MODIFIER.is_match(&outside_strings(line)) {
                    return Err("unsupported source URL arguments".into());
                }
                if let Some(version) = declared_tag(line) {
                    urls[index].1 = version;
                }
                continue;
            }
            pending_url = None;
        }
        if EXCLUDED.is_match(line) {
            skip_indent = Some(indent);
            continue;
        }
        while scopes.last().is_some_and(|(start, _)| *start >= indent) {
            scopes.pop();
        }
        if BLOCK.is_match(line)
            || ["class ", "if ", "unless ", "case ", "when ", "elsif "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            || matches!(line, "else" | "begin")
        {
            scopes.push((indent, next_scope));
            next_scope += 1;
        }
        let scope: Vec<_> = scopes.iter().map(|(_, id)| *id).collect();
        if let Some(captures) = ATTRIBUTE.captures(line) {
            let value = captures[2].to_string();
            let tail = line[captures[0].len()..].trim();
            if &captures[1] == "version" {
                if !tail.is_empty() {
                    return Err("unsupported version declaration".into());
                }
                versions.push((scope, value));
            } else {
                if (!tail.is_empty() && !tail.starts_with(','))
                    || MODIFIER.is_match(&outside_strings(tail))
                {
                    return Err("unsupported source URL declaration".into());
                }
                urls.push((
                    scope,
                    declared_tag(tail).unwrap_or_else(|| source_version(&value)),
                ));
                if line.trim_end().ends_with(',') {
                    pending_url = Some((indent, urls.len() - 1));
                }
            }
        } else if DECLARATION.is_match(line) {
            return Err("unsupported version or source URL declaration".into());
        }
    }
    if heredoc.is_some() || comment {
        return Err("unterminated heredoc or block comment".into());
    }
    if versions.iter().any(|(_, v)| v.contains("#{")) {
        return Err("dynamic version declaration".into());
    }
    // An explicit version applies only to sources in its own/enclosed block.
    // In particular, a macOS version cannot mask an older Linux source URL.
    if urls.is_empty() {
        return unique_version(
            versions
                .into_iter()
                .map(|(_, v)| normalize_version(v))
                .collect(),
        );
    }
    unique_version(
        urls.into_iter()
            .map(|(scope, inferred)| {
                let nearest = versions
                    .iter()
                    .filter(|(parent, _)| scope.starts_with(parent))
                    .map(|(parent, _)| parent.len())
                    .max();
                let explicit = versions
                    .iter()
                    .filter(|(parent, _)| {
                        Some(parent.len()) == nearest && scope.starts_with(parent)
                    })
                    .map(|(_, v)| normalize_version(v.clone()))
                    .collect::<Vec<_>>();
                if !explicit.is_empty() {
                    return unique_version(explicit);
                }
                inferred.ok_or_else(|| {
                    "cannot infer stable source version; declare an explicit version".into()
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    )
}

// Outer None means no tag argument; inner None means a tag exists but is not
// a static version. Never silently fall back to the URL for a dynamic tag.
fn declared_tag(arguments: &str) -> Option<Option<String>> {
    if !TAG_KEY.is_match(arguments) {
        return None;
    }
    Some(TAG.captures(arguments).and_then(|tag| {
        let tail = arguments[tag.get(0)?.end()..].trim();
        if !tail.is_empty() && !tail.starts_with(',') {
            return None;
        }
        VERSION_SEGMENT.captures(&tag[1]).map(|v| v[1].to_string())
    }))
}

fn normalize_version(version: String) -> String {
    version.strip_prefix('v').unwrap_or(&version).to_string()
}

fn heredoc_marker(line: &str) -> Result<Option<(String, bool)>, String> {
    let mut quote = None;
    let mut escaped = false;
    let mut found = None;
    for (i, ch) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quote.is_some() {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() {
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
            } else if ch == '<'
                && let Some(capture) = HEREDOC.captures(&line[i..])
            {
                if found.is_some() {
                    return Err("multiple heredocs on one line are unsupported".into());
                }
                found = Some((
                    capture[1].to_string(),
                    line[i..].starts_with("<<-") || line[i..].starts_with("<<~"),
                ));
            }
        }
    }
    Ok(found)
}

fn outside_strings(line: &str) -> String {
    let mut quote = None;
    let mut escaped = false;
    line.chars()
        .map(|ch| {
            if escaped {
                escaped = false;
                ' '
            } else if ch == '\\' && quote.is_some() {
                escaped = true;
                ' '
            } else if quote == Some(ch) {
                quote = None;
                ' '
            } else if quote.is_some() {
                ' '
            } else if ch == '\'' || ch == '"' {
                quote = Some(ch);
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quote.is_some() {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() {
            if ch == '#' {
                return &line[..i];
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
            }
        }
    }
    line
}

fn unique_version(mut versions: Vec<String>) -> Result<String, String> {
    versions.sort();
    versions.dedup();
    match versions.len() {
        1 => Ok(versions.remove(0)),
        0 => Err("no static stable version or source URL".into()),
        _ => Err("conflicting stable versions".into()),
    }
}

fn source_version(url: &str) -> Option<String> {
    if url.contains("#{") {
        return None;
    }
    let path = url.split(['?', '#']).next()?;
    // Exclude the scheme and authority, including numeric hostnames.
    let path = path.split_once("://")?.1.split_once('/')?.1;
    let (parents, name) = path.rsplit_once('/').unwrap_or(("", path));
    let name = [
        ".tar.gz",
        ".tar.bz2",
        ".tar.xz",
        ".tar.zst",
        ".tar.lz",
        ".tar.lzma",
        ".tar.Z",
        ".tgz",
        ".tbz2",
        ".txz",
        ".zip",
        ".7z",
        ".jar",
        ".tar",
        ".crate",
        ".gem",
    ]
    .iter()
    .find_map(|suffix| name.strip_suffix(suffix))
    .unwrap_or(name);
    // Source filenames are more precise than directory series, e.g.
    // /glib/2.78/glib-2.78.3.tar.xz. Release tags cover platform binaries
    // whose filenames carry OS/architecture suffixes instead.
    let tokens: Vec<_> = VERSION_TOKEN.find_iter(name).map(|m| m.as_str()).collect();
    if tokens.len() == 1
        && let Some(capture) = FILE_VERSION.captures(name)
    {
        return Some(capture[1].to_string());
    }
    let parent = parents
        .split('/')
        .rev()
        .find_map(|part| VERSION_SEGMENT.captures(part).map(|c| c[1].to_string()))?;
    let core = VERSION_TOKEN.find(&parent)?.as_str();
    // Multiple numeric tokens can include an OS/libc version (Ubuntu 22.04,
    // glibc 2.17). Trust a directory tag only if it agrees with a filename
    // token; otherwise these filenames require an explicit version.
    (tokens.is_empty() || tokens.contains(&core)).then_some(parent)
}
