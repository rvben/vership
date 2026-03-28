use regex::Regex;
use semver::Version;
use serde::Deserialize;

use crate::cli::BumpLevel;
use crate::error::{Error, Result};

pub fn bump(version: Version, level: BumpLevel) -> Version {
    match level {
        BumpLevel::Patch => Version::new(version.major, version.minor, version.patch + 1),
        BumpLevel::Minor => Version::new(version.major, version.minor + 1, 0),
        BumpLevel::Major => Version::new(version.major + 1, 0, 0),
    }
}

#[derive(Deserialize)]
struct CargoToml {
    package: CargoPackage,
}

#[derive(Deserialize)]
struct CargoPackage {
    version: String,
}

pub fn parse_cargo_toml_version(content: &str) -> Result<Version> {
    let parsed: CargoToml = toml::from_str(content)
        .map_err(|e| Error::Version(format!("failed to parse Cargo.toml: {e}")))?;
    Version::parse(&parsed.package.version)
        .map_err(|e| Error::Version(format!("invalid version '{}': {e}", parsed.package.version)))
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
    dynamic: Option<Vec<String>>,
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
