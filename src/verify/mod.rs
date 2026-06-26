pub mod checkers;
pub mod report;
pub mod targets;

use std::path::Path;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::output::OutputConfig;

/// Run post-release verification for `version` (defaults to the on-disk version).
pub fn run(
    version: Option<&str>,
    only: Option<&str>,
    skip: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    let root = Path::new(".");
    let config = Config::load(Path::new("vership.toml"));

    let version = resolve_version(root, &config, version)?;
    let tag = format!("v{version}");

    let remote = crate::git::remote_url(root)?;
    // Best-effort: a project type whose only published artifact is the git tag
    // (e.g. an Ansible collection) verifies tag-only. Detection failures fall
    // back to the default (full target detection).
    let tag_only = crate::project::detect(root, config.project.project_type.as_deref())
        .map(|p| p.publishes_only_git_tag())
        .unwrap_or(false);
    let detected = targets::detect_targets(root, &config.verify, remote.as_deref(), tag_only)?;
    let targets = targets::filter_targets(detected, only, skip)?;
    if targets.is_empty() {
        return Err(Error::Config(
            "no verify targets detected or selected".to_string(),
        ));
    }

    let agent = checkers::default_agent();
    let reports: Vec<TargetReport> = targets
        .iter()
        .map(|target| {
            let result = match target {
                targets::Target::Tag => match crate::git::remote_tag_exists(root, &tag) {
                    Ok(true) => CheckResult::Found(tag.clone()),
                    Ok(false) => CheckResult::NotFound,
                    Err(e) => CheckResult::Error(e.to_string()),
                },
                targets::Target::Release => checkers::release(root, &tag, &version),
                targets::Target::Crates { name } => {
                    checkers::crates(&agent, checkers::CRATES_IO, name, &version)
                }
                targets::Target::Pypi { name } => {
                    checkers::pypi(&agent, checkers::PYPI, name, &version)
                }
                targets::Target::Npm { name } => {
                    checkers::npm(&agent, checkers::NPM, name, &version)
                }
                targets::Target::Homebrew { tap, formula } => {
                    checkers::homebrew(&agent, checkers::RAW_GITHUB, tap, formula, &version)
                }
                targets::Target::Ghcr { image } => {
                    let cred = checkers::resolve_ghcr_credential();
                    checkers::ghcr(&agent, checkers::GHCR, image, &version, cred.as_ref())
                }
            };
            TargetReport::from_result(target.name(), result)
        })
        .collect();

    report::render(&version, &reports, output);

    let failed: Vec<&str> = reports
        .iter()
        .filter(|r| !r.ok)
        .map(|r| r.name.as_str())
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Error::Unpublished(format!(
            "{version} missing on: {}",
            failed.join(", ")
        )))
    }
}

/// Explicit version arg (with or without leading v) wins; otherwise the
/// on-disk project version; otherwise the latest semver tag.
fn resolve_version(root: &Path, config: &Config, explicit: Option<&str>) -> Result<String> {
    if let Some(v) = explicit {
        return Ok(v.trim_start_matches('v').to_string());
    }
    let project_type = config.project.project_type.as_deref();
    if let Ok(project) = crate::project::detect(root, project_type)
        && let Ok(version) = project.read_version(root)
    {
        return Ok(version.to_string());
    }
    if let Some(tag) = crate::git::latest_semver_tag(root)? {
        return Ok(tag.trim_start_matches('v').to_string());
    }
    Err(Error::Version(
        "could not determine version to verify: pass it explicitly".to_string(),
    ))
}

/// Result of checking one target for a specific version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The exact expected version is live.
    Found(String),
    /// The target exists but serves a different version.
    FoundOld(String),
    /// The target has no trace of the package or version.
    NotFound,
    /// The check itself could not complete (network, auth).
    Error(String),
}

/// One row of the verification report.
#[derive(Debug)]
pub struct TargetReport {
    pub name: String,
    pub ok: bool,
    pub found: Option<String>,
    pub detail: Option<String>,
}

impl TargetReport {
    pub fn from_result(name: &str, result: CheckResult) -> Self {
        match result {
            CheckResult::Found(v) => TargetReport {
                name: name.to_string(),
                ok: true,
                found: Some(v),
                detail: None,
            },
            CheckResult::FoundOld(v) => TargetReport {
                name: name.to_string(),
                ok: false,
                found: Some(v.clone()),
                detail: Some(format!("found {v} instead")),
            },
            CheckResult::NotFound => TargetReport {
                name: name.to_string(),
                ok: false,
                found: None,
                detail: Some("not found".to_string()),
            },
            CheckResult::Error(e) => TargetReport {
                name: name.to_string(),
                ok: false,
                found: None,
                detail: Some(format!("check error: {e}")),
            },
        }
    }
}
