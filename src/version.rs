use regex::Regex;
use semver::Version;

use crate::cli::BumpLevel;
use crate::error::{Error, Result};

pub fn bump(version: Version, level: BumpLevel) -> Version {
    match level {
        BumpLevel::Patch => Version::new(version.major, version.minor, version.patch + 1),
        BumpLevel::Minor => Version::new(version.major, version.minor + 1, 0),
        BumpLevel::Major => Version::new(version.major + 1, 0, 0),
    }
}

pub fn parse_cargo_toml_version(content: &str) -> Result<Version> {
    let re = Regex::new(r#"(?m)^version\s*=\s*"([^"]+)""#).expect("valid regex");
    let caps = re
        .captures(content)
        .ok_or_else(|| Error::Version("no version field found in Cargo.toml".to_string()))?;
    let version_str = &caps[1];
    Version::parse(version_str)
        .map_err(|e| Error::Version(format!("invalid version '{version_str}': {e}")))
}

pub fn replace_cargo_toml_version(content: &str, new_version: &Version) -> String {
    let re = Regex::new(r#"(?m)^(version\s*=\s*")[^"]+(")"#).expect("valid regex");
    re.replace(content, format!("${{1}}{new_version}${{2}}"))
        .to_string()
}

/// Replace version in pyproject.toml if a static version field exists.
/// Returns None if the version is dynamic (managed by maturin).
pub fn replace_pyproject_version(content: &str, new_version: &Version) -> Option<String> {
    let re = Regex::new(r#"(?m)^version\s*=\s*"[^"]+""#).expect("valid regex");
    // Only replace if there's a static version and "version" is not in dynamic list
    if content.contains(r#""version""#)
        && content.contains("[project]")
        && content.contains("dynamic")
        && content.contains("version")
    {
        // version is dynamic, don't replace
        return None;
    }
    if re.is_match(content) {
        Some(
            re.replace(content, format!(r#"version = "{new_version}""#))
                .to_string(),
        )
    } else {
        None
    }
}
