use regex::Regex;
use semver::Version;
use serde::Deserialize;

use crate::cli::BumpLevel;
use crate::error::{Error, Result};

#[derive(Deserialize)]
struct PackageJson {
    version: Option<String>,
}

pub fn parse_package_json_version(content: &str) -> Result<Version> {
    let parsed: PackageJson = serde_json::from_str(content)
        .map_err(|e| Error::Version(format!("failed to parse package.json: {e}")))?;
    let version_str = parsed
        .version
        .ok_or_else(|| Error::Version("no version field in package.json".to_string()))?;
    Version::parse(&version_str)
        .map_err(|e| Error::Version(format!("invalid version '{version_str}': {e}")))
}

pub fn replace_package_json_version(content: &str, new_version: &Version) -> String {
    let re = Regex::new(r#"("version"\s*:\s*")[^"]+"#).expect("valid regex");
    re.replace(content, format!("${{1}}{new_version}"))
        .to_string()
}

/// Read the value of a `key=value` (or `key: value`) entry in a
/// gradle.properties file. Matching is anchored to the start of a line so
/// `version` never matches `pluginVersion`.
pub fn parse_gradle_property(content: &str, key: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?m)^\s*{}\s*[=:]\s*(\S+)", regex::escape(key)))
        .expect("valid regex");
    re.captures(content).map(|c| c[1].trim().to_string())
}

/// Replace the value of `key` in a gradle.properties file, preserving the
/// surrounding key, separator, and other lines. Returns None if the key is absent.
pub fn replace_gradle_property(content: &str, key: &str, new_version: &Version) -> Option<String> {
    let re = Regex::new(&format!(r"(?m)^(\s*{}\s*[=:]\s*)\S+", regex::escape(key)))
        .expect("valid regex");
    if re.is_match(content) {
        Some(
            re.replace(content, format!("${{1}}{new_version}"))
                .to_string(),
        )
    } else {
        None
    }
}

/// Read a `version = "x.y.z"` (or single-quoted, or no `=` Groovy form)
/// assignment from a build.gradle / build.gradle.kts script.
pub fn parse_gradle_buildscript_version(content: &str) -> Option<String> {
    let re = Regex::new(r#"(?m)^\s*version\s*=?\s*["']([^"']+)["']"#).expect("valid regex");
    re.captures(content).map(|c| c[1].to_string())
}

/// Replace the `version` assignment in a build.gradle / build.gradle.kts script.
/// Returns None if no such assignment exists.
pub fn replace_gradle_buildscript_version(content: &str, new_version: &Version) -> Option<String> {
    let re = Regex::new(r#"(?m)^(\s*version\s*=?\s*["'])[^"']+(["'])"#).expect("valid regex");
    if re.is_match(content) {
        Some(
            re.replace(content, format!("${{1}}{new_version}${{2}}"))
                .to_string(),
        )
    } else {
        None
    }
}

/// Read a top-level scalar field (e.g. `namespace`, `name`, `version`) from a
/// `galaxy.yml`, stripping surrounding quotes and a trailing inline comment.
///
/// Matching is anchored to column 0 so only top-level mapping keys are read,
/// never a nested `version:` inside another block. The `regex` crate has no
/// backreferences, so the opening and closing quotes are captured as
/// independent groups and the field is rejected (returns None) unless they
/// balance: an unterminated or mismatched quote (`version: "0.0.2`) is a
/// malformed manifest, not a value to be silently accepted.
pub fn parse_galaxy_field(content: &str, key: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?m)^{}[ \t]*:[ \t]*(["']?)([^"'\r\n#]+?)(["']?)[ \t]*(?:#[^\r\n]*)?\r?$"#,
        regex::escape(key)
    ))
    .expect("valid regex");
    let caps = re.captures(content)?;
    if caps[1] != caps[3] {
        return None;
    }
    Some(caps[2].trim().to_string())
}

/// Surgically rewrite only the top-level `version:` line in a `galaxy.yml`,
/// preserving the surrounding quote style (`"..."`, `'...'`, or unquoted) and
/// any trailing inline comment. Returns None if no `version:` line is present
/// or its quoting is unbalanced (a malformed manifest is never rewritten).
pub fn replace_galaxy_version(content: &str, new_version: &Version) -> Option<String> {
    let re = Regex::new(
        r#"(?m)^(version[ \t]*:[ \t]*)(["']?)[^"'\r\n#]+?(["']?)([ \t]*(?:#[^\r\n]*)?\r?)$"#,
    )
    .expect("valid regex");
    let caps = re.captures(content)?;
    if caps[2] != caps[3] {
        return None;
    }
    Some(
        re.replace(content, |caps: &regex::Captures| {
            format!(
                "{}{}{}{}{}",
                &caps[1], &caps[2], new_version, &caps[3], &caps[4]
            )
        })
        .to_string(),
    )
}

pub fn bump(version: Version, level: BumpLevel) -> Version {
    match level {
        BumpLevel::Patch => Version::new(version.major, version.minor, version.patch + 1),
        BumpLevel::Minor => Version::new(version.major, version.minor + 1, 0),
        BumpLevel::Major => Version::new(version.major + 1, 0, 0),
    }
}

#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
    workspace: Option<CargoWorkspace>,
}

#[derive(Deserialize)]
struct CargoWorkspace {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    // `version` may be an explicit string (`version = "1.2.3"`) or a workspace
    // inheritance table (`version.workspace = true`). Capture it loosely and
    // extract the string form only when present.
    version: Option<toml::Value>,
}

pub fn parse_cargo_toml_version(content: &str) -> Result<Version> {
    let parsed: CargoToml = toml::from_str(content)
        .map_err(|e| Error::Version(format!("failed to parse Cargo.toml: {e}")))?;

    // Prefer an explicit `[package].version` string; fall back to
    // `[workspace.package].version`. This supports both single-crate manifests
    // and pure-workspace roots (no `[package]` table), as well as member crates
    // that inherit the version via `version.workspace = true`.
    let version_str = parsed
        .package
        .as_ref()
        .and_then(|p| p.version.as_ref())
        .and_then(|v| v.as_str())
        .or_else(|| {
            parsed
                .workspace
                .as_ref()
                .and_then(|w| w.package.as_ref())
                .and_then(|p| p.version.as_ref())
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| {
            Error::Version(
                "no [package].version or [workspace.package].version in Cargo.toml".to_string(),
            )
        })?;

    Version::parse(version_str)
        .map_err(|e| Error::Version(format!("invalid version '{version_str}': {e}")))
}

pub fn replace_cargo_toml_version(content: &str, new_version: &Version) -> String {
    let re = Regex::new(r#"(?m)^(version\s*=\s*")[^"]+(")"#).expect("valid regex");
    re.replace(content, format!("${{1}}{new_version}${{2}}"))
        .to_string()
}

#[derive(Deserialize)]
struct PyprojectToml {
    project: Option<PyprojectProject>,
}

#[derive(Deserialize)]
struct PyprojectProject {
    version: Option<String>,
    dynamic: Option<Vec<String>>,
}

pub fn parse_pyproject_version(content: &str) -> Result<Version> {
    let parsed: PyprojectToml = toml::from_str(content)
        .map_err(|e| Error::Version(format!("failed to parse pyproject.toml: {e}")))?;
    let project = parsed
        .project
        .ok_or_else(|| Error::Version("no [project] section in pyproject.toml".to_string()))?;
    if let Some(dynamic) = &project.dynamic
        && dynamic.iter().any(|s| s == "version")
    {
        return Err(Error::Version(
            "version is listed in dynamic, cannot read static version".to_string(),
        ));
    }
    let version_str = project
        .version
        .ok_or_else(|| Error::Version("no version field in [project] section".to_string()))?;
    Version::parse(&version_str)
        .map_err(|e| Error::Version(format!("invalid version '{version_str}': {e}")))
}

/// Replace version in pyproject.toml if a static version field exists.
/// Returns None if the version is dynamic (listed in `[project].dynamic`).
pub fn replace_pyproject_version(content: &str, new_version: &Version) -> Option<String> {
    let parsed: PyprojectToml = toml::from_str(content).ok()?;
    if let Some(project) = parsed.project
        && let Some(dynamic) = project.dynamic
        && dynamic.iter().any(|s| s == "version")
    {
        return None;
    }

    let re = Regex::new(r#"(?m)^version\s*=\s*"[^"]+""#).expect("valid regex");
    if re.is_match(content) {
        Some(
            re.replace(content, format!(r#"version = "{new_version}""#))
                .to_string(),
        )
    } else {
        None
    }
}
