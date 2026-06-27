use std::path::Path;

use serde::Deserialize;

use crate::config::VerifyConfig;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Tag,
    Release,
    Crates { name: String },
    Pypi { name: String },
    Npm { name: String },
    Homebrew { tap: String, formulas: Vec<String> },
    Ghcr { image: String },
}

impl Target {
    pub fn name(&self) -> &'static str {
        match self {
            Target::Tag => "tag",
            Target::Release => "release",
            Target::Crates { .. } => "crates",
            Target::Pypi { .. } => "pypi",
            Target::Npm { .. } => "npm",
            Target::Homebrew { .. } => "homebrew",
            Target::Ghcr { .. } => "ghcr",
        }
    }
}

/// Candidate Homebrew formula names to probe, in priority order. A formula is
/// conventionally named after the installed binary (the crate name for a Rust
/// tool), which is not always the repository name — e.g. the `clispec-cli` repo
/// ships a `clispec` formula. An explicit `formula` config wins outright;
/// otherwise try the crate name then the repo name, deduplicated, and use
/// whichever formula actually exists in the tap.
fn formula_candidates(
    config_formula: Option<&str>,
    crate_name: Option<&str>,
    repo: &str,
) -> Vec<String> {
    if let Some(f) = config_formula {
        return vec![f.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for name in [crate_name, Some(repo)].into_iter().flatten() {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[derive(Deserialize)]
struct CargoManifest {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: Option<String>,
    publish: Option<toml::Value>,
}

#[derive(Deserialize)]
struct Pyproject {
    project: Option<PyprojectProject>,
}

#[derive(Deserialize)]
struct PyprojectProject {
    name: Option<String>,
}

#[derive(Deserialize)]
struct PackageJson {
    name: Option<String>,
    private: Option<bool>,
}

/// Extract "owner/repo" from a normalized https remote URL.
fn owner_repo(remote_url: &str) -> Option<(String, String)> {
    let path = remote_url.strip_prefix("https://github.com/")?;
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Concatenated content of all workflow files, for publish-step detection.
fn workflows_content(root: &Path) -> String {
    let dir = root.join(".github/workflows");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return String::new();
    };
    let mut content = String::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = path.extension().is_some_and(|e| e == "yml" || e == "yaml");
        if is_yaml && let Ok(text) = std::fs::read_to_string(&path) {
            content.push_str(&text);
            content.push('\n');
        }
    }
    content
}

/// Detect the publish targets for the repo at `root`.
///
/// `remote_url` is the normalized origin URL (from `git::remote_url`); GitHub
/// targets (tag, release, homebrew defaults, ghcr defaults) require it.
///
/// `tag_only` is the project type's default (`ProjectType::
/// publishes_only_git_tag`): when true (e.g. an Ansible collection consumed by
/// git ref), the git tag is the entire release, so detection stops at the tag
/// and never adds a GitHub Release or any registry target inferred from
/// incidental package metadata (a tooling-only `pyproject.toml`, a companion
/// `Cargo.toml`, etc.).
pub fn detect_targets(
    root: &Path,
    config: &VerifyConfig,
    remote_url: Option<&str>,
    tag_only: bool,
) -> Result<Vec<Target>> {
    let github = remote_url.and_then(owner_repo);
    let mut targets = Vec::new();

    if tag_only {
        // The git tag is the entire release. The remote tag check uses
        // `git ls-remote origin`, which works for any Git host, so the tag
        // target needs only a remote, not specifically a GitHub one. This
        // matters: collections are commonly hosted on GitLab / internal Git.
        // Nothing else is published, so detection stops here.
        if remote_url.is_some() {
            targets.push(Target::Tag);
        }
        targets.retain(|t| !config.skip.iter().any(|s| s == t.name()));
        return Ok(targets);
    }

    if github.is_some() {
        targets.push(Target::Tag);
        targets.push(Target::Release);
    }

    let cargo_path = root.join("Cargo.toml");
    // Captured for the Homebrew formula default: the formula is named after the
    // installed binary (the crate name), which can differ from the repo name.
    let mut crate_name: Option<String> = None;
    if cargo_path.exists() {
        let content = std::fs::read_to_string(&cargo_path)?;
        let manifest: CargoManifest = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("parse Cargo.toml: {e}")))?;
        if let Some(package) = manifest.package {
            crate_name = package.name.clone();
            // `publish = false` opts out entirely; a registry list restricts
            // publication to the named registries, so crates.io is a target
            // only when the list names it ("crates-io").
            let publishable = match &package.publish {
                None => true,
                Some(toml::Value::Boolean(b)) => *b,
                Some(toml::Value::Array(registries)) => {
                    registries.iter().any(|r| r.as_str() == Some("crates-io"))
                }
                Some(_) => true,
            };
            if publishable && let Some(name) = package.name {
                targets.push(Target::Crates { name });
            }
        }
    }

    let pyproject_path = root.join("pyproject.toml");
    if pyproject_path.exists() {
        let content = std::fs::read_to_string(&pyproject_path)?;
        let pyproject: Pyproject = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("parse pyproject.toml: {e}")))?;
        if let Some(name) = pyproject.project.and_then(|p| p.name) {
            targets.push(Target::Pypi { name });
        }
    }

    let package_json_path = root.join("package.json");
    if package_json_path.exists() {
        let content = std::fs::read_to_string(&package_json_path)?;
        let package: PackageJson = serde_json::from_str(&content)
            .map_err(|e| Error::Config(format!("parse package.json: {e}")))?;
        if package.private != Some(true)
            && let Some(name) = package.name
        {
            targets.push(Target::Npm { name });
        }
    }

    let workflows = workflows_content(root);
    if let Some((owner, repo)) = &github {
        if workflows.contains("homebrew-tap") || config.tap.is_some() {
            targets.push(Target::Homebrew {
                tap: config
                    .tap
                    .clone()
                    .unwrap_or_else(|| format!("{owner}/homebrew-tap")),
                formulas: formula_candidates(
                    config.formula.as_deref(),
                    crate_name.as_deref(),
                    repo,
                ),
            });
        }
        if workflows.contains("ghcr.io") || config.image.is_some() {
            targets.push(Target::Ghcr {
                image: config
                    .image
                    .clone()
                    .unwrap_or_else(|| format!("{owner}/{repo}").to_lowercase()),
            });
        }
    }

    targets.retain(|t| !config.skip.iter().any(|s| s == t.name()));
    Ok(targets)
}

/// Apply --targets / --skip CLI filters (comma-separated names).
pub fn filter_targets(
    targets: Vec<Target>,
    only: Option<&str>,
    skip: Option<&str>,
) -> Result<Vec<Target>> {
    const VALID: [&str; 7] = [
        "tag", "release", "crates", "pypi", "npm", "homebrew", "ghcr",
    ];
    let parse = |list: &str| -> Result<Vec<String>> {
        list.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if VALID.contains(&s) {
                    Ok(s.to_string())
                } else {
                    Err(Error::Config(format!(
                        "unknown target '{s}': valid targets are {}",
                        VALID.join(", ")
                    )))
                }
            })
            .collect()
    };
    let mut targets = targets;
    if let Some(only) = only {
        let keep = parse(only)?;
        targets.retain(|t| keep.iter().any(|k| k == t.name()));
    }
    if let Some(skip) = skip {
        let drop = parse(skip)?;
        targets.retain(|t| !drop.iter().any(|k| k == t.name()));
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_candidates_prefers_crate_then_repo() {
        // repo != crate (the clispec-cli case): probe the crate name first, then
        // the repo name, so a `clispec` formula in a `clispec-cli` repo is found.
        let c = formula_candidates(None, Some("clispec"), "clispec-cli");
        assert_eq!(c, vec!["clispec".to_string(), "clispec-cli".to_string()]);
    }

    #[test]
    fn formula_candidates_dedupes_when_crate_equals_repo() {
        let c = formula_candidates(None, Some("foo"), "foo");
        assert_eq!(c, vec!["foo".to_string()]);
    }

    #[test]
    fn formula_candidates_honors_explicit_config() {
        let c = formula_candidates(Some("custom"), Some("foo"), "bar");
        assert_eq!(c, vec!["custom".to_string()]);
    }

    #[test]
    fn formula_candidates_falls_back_to_repo_without_crate() {
        let c = formula_candidates(None, None, "bar");
        assert_eq!(c, vec!["bar".to_string()]);
    }
}
