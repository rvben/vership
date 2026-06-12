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
    Homebrew { tap: String, formula: String },
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
pub fn detect_targets(
    root: &Path,
    config: &VerifyConfig,
    remote_url: Option<&str>,
) -> Result<Vec<Target>> {
    let github = remote_url.and_then(owner_repo);
    let mut targets = Vec::new();

    if github.is_some() {
        targets.push(Target::Tag);
        targets.push(Target::Release);
    }

    let cargo_path = root.join("Cargo.toml");
    if cargo_path.exists() {
        let content = std::fs::read_to_string(&cargo_path)?;
        let manifest: CargoManifest = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("parse Cargo.toml: {e}")))?;
        if let Some(package) = manifest.package {
            // `publish = false` opts out; `publish = ["registry"]` or absent
            // both mean publishable.
            let publishable = !matches!(&package.publish, Some(toml::Value::Boolean(false)));
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
                formula: config.formula.clone().unwrap_or_else(|| repo.clone()),
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
