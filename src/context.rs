//! One explicit project selection shared by every repository operation.
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use semver::Version;
use serde::Deserialize;

use crate::{
    config::Config,
    error::{Error, Result},
    git,
    project::ProjectType,
};

#[derive(Default, Deserialize)]
struct Registry {
    #[serde(default)]
    projects: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: PathBuf,
    tag_prefix: String,
    /// Literal repository-relative files or directories; overlaps are intentional.
    paths: Option<Vec<PathBuf>>,
}

pub(crate) struct ReleaseContext {
    pub repo: PathBuf,
    pub root: PathBuf,
    pub name: Option<String>,
    pub prefix: String,
    pub config: Config,
    pub verify_targets: Option<Vec<String>>,
    pub independent_roots: Vec<PathBuf>,
    includes: Option<Vec<PathBuf>>,
}

fn config_error(message: impl Into<String>) -> Error {
    Error::Config(message.into())
}

pub(crate) fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(config_error(format!(
            "expected a relative path without '..': {}",
            path.display()
        )));
    }
    Ok(())
}

fn read_config(path: &Path) -> Result<toml::Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).map_err(|e| config_error(format!("{}: {e}", path.display())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(e) => Err(e.into()),
    }
}

impl ReleaseContext {
    pub fn resolve(selector: Option<&str>) -> Result<Self> {
        Self::at(&std::env::current_dir()?, selector)
    }

    pub fn at(cwd: &Path, selector: Option<&str>) -> Result<Self> {
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_SHALLOW_FILE",
        ] {
            if std::env::var_os(var).is_some() {
                return Err(config_error(format!(
                    "{var} overrides repository identity; unset it before running vership"
                )));
            }
        }
        let cwd = cwd.canonicalize()?;
        let repo = git::repository_root(&cwd).unwrap_or_else(|_| cwd.clone());
        let value = read_config(&repo.join("vership.toml"))?;
        let registry: Registry = value
            .clone()
            .try_into()
            .map_err(|e| config_error(format!("project registry: {e}")))?;
        let mut entries = Vec::new();
        let mut prefixes = vec!["v".to_string()];
        for (name, entry) in &registry.projects {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Err(config_error(format!("invalid project name '{name}'")));
            }
            relative(&entry.path)?;
            let root = repo
                .join(&entry.path)
                .canonicalize()
                .map_err(|e| config_error(format!("project '{name}': {e}")))?;
            if root == repo
                || !root.starts_with(&repo)
                || entries
                    .iter()
                    .any(|(_, p): &(&String, PathBuf)| root.starts_with(p) || p.starts_with(&root))
            {
                return Err(config_error(format!(
                    "project '{name}' has an escaped, duplicate, or nested root"
                )));
            }
            if !entry
                .tag_prefix
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric())
                || !entry
                    .tag_prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
                || !git::valid_tag(&repo, &format!("{}0.0.0", entry.tag_prefix))?
            {
                return Err(config_error(format!(
                    "invalid tag_prefix for project '{name}'"
                )));
            }
            if prefixes.contains(&entry.tag_prefix) {
                return Err(config_error(format!(
                    "duplicate tag_prefix '{}'",
                    entry.tag_prefix
                )));
            }
            // Prefixes may contain each other only if no semantic version could be
            // parsed in both namespaces (v and vendor-v are safe, v and v1. are not).
            for prefix in &prefixes {
                let (short, long) = if prefix.len() < entry.tag_prefix.len() {
                    (prefix.as_str(), entry.tag_prefix.as_str())
                } else {
                    (entry.tag_prefix.as_str(), prefix.as_str())
                };
                if let Some(tail) = long.strip_prefix(short)
                    && tail.chars().next().is_some_and(|c| c.is_ascii_digit())
                {
                    return Err(config_error("potentially ambiguous tag namespaces"));
                }
            }
            prefixes.push(entry.tag_prefix.clone());
            if !root.join("vership.toml").is_file() {
                return Err(config_error(format!(
                    "project '{name}' requires its own vership.toml"
                )));
            }
            if let Some(paths) = &entry.paths {
                if paths.is_empty() {
                    return Err(config_error(format!(
                        "project '{name}' paths cannot be empty"
                    )));
                }
                for path in paths {
                    relative(path)?;
                    Self::contained(&repo, path)?;
                }
            }
            entries.push((name, root));
        }
        let independent_roots: Vec<_> = entries.iter().map(|(_, p)| p.clone()).collect();
        let (root, prefix, includes) = if let Some(name) = selector {
            let entry = registry.projects.get(name).ok_or_else(|| {
                config_error(format!(
                    "unknown project '{name}'; available: {}",
                    registry
                        .projects
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            let root = entries
                .iter()
                .find(|(n, _)| n.as_str() == name)
                .unwrap()
                .1
                .clone();
            (
                root.clone(),
                entry.tag_prefix.clone(),
                Some(
                    entry
                        .paths
                        .clone()
                        .unwrap_or_else(|| {
                            vec![
                                root.strip_prefix(&repo)
                                    .expect("registered root is contained")
                                    .to_path_buf(),
                            ]
                        })
                        .into_iter()
                        .map(|path| {
                            path.components()
                                .filter_map(|c| {
                                    if let Component::Normal(p) = c {
                                        Some(p)
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        })
                        .collect(),
                ),
            )
        } else if !entries.is_empty() {
            if let Some((name, _)) = entries.iter().find(|(_, root)| cwd.starts_with(root)) {
                return Err(config_error(format!(
                    "inside registered project '{name}'; select it explicitly with --project {name}"
                )));
            }
            (repo.clone(), "v".into(), None)
        } else {
            // Preserve historical cwd-based behavior in repositories without a registry.
            (cwd.clone(), "v".into(), None)
        };
        let mut config = Config::load_checked(&root.join("vership.toml"))?;
        let child_value = read_config(&root.join("vership.toml"))?;
        let root_config = Config::load_checked(&repo.join("vership.toml"))?;
        if selector.is_some() {
            if child_value.get("projects").is_some() {
                return Err(config_error("nested project registries are unsupported"));
            }
            if let Some(branch) = child_value
                .get("project")
                .and_then(|p| p.get("branch"))
                .and_then(toml::Value::as_str)
                && branch != root_config.project.branch
            {
                return Err(config_error(
                    "child project branch must match the repository branch",
                ));
            }
            config.project.branch = root_config.project.branch;
        }
        let verify_targets = child_value
            .get("verify")
            .and_then(|v| v.get("targets"))
            .map(|v| {
                v.clone()
                    .try_into::<Vec<String>>()
                    .map_err(|e| config_error(format!("verify.targets: {e}")))
            })
            .transpose()?;
        if selector.is_some() && verify_targets.as_ref().is_none_or(|t| t.is_empty()) {
            return Err(config_error(
                "named projects must explicitly declare nonempty verify.targets",
            ));
        }
        if let Some(declared) = &verify_targets {
            if declared.is_empty() {
                return Err(config_error("verify.targets cannot be empty"));
            }
            let mut seen = std::collections::BTreeSet::new();
            for target in declared {
                crate::verify::targets::known_target(target)?;
                if !seen.insert(target) {
                    return Err(config_error(format!(
                        "verify.targets: duplicate target '{target}'"
                    )));
                }
            }
            for target in &config.verify.skip {
                crate::verify::targets::known_target(target)
                    .map_err(|e| config_error(format!("verify.skip: {e}")))?;
            }
        }
        let ctx = Self {
            repo,
            root,
            name: selector.map(str::to_owned),
            prefix,
            config,
            verify_targets,
            independent_roots,
            includes,
        };
        if ctx.name.is_some() && ctx.root.join("Cargo.toml").exists() {
            let local = read_config(&ctx.root.join("Cargo.toml"))?;
            if local.get("workspace").is_none() {
                let mut parent = ctx.root.parent();
                while let Some(dir) = parent {
                    if !dir.starts_with(&ctx.repo) {
                        break;
                    }
                    if read_config(&dir.join("Cargo.toml"))?
                        .get("workspace")
                        .is_some()
                    {
                        return Err(config_error(
                            "independent Cargo projects must own their workspace and lockfile; inherited parent workspaces are unsupported",
                        ));
                    }
                    parent = dir.parent();
                }
            }
        }

        if ctx.name.is_some() && ctx.root.join("package.json").exists() {
            let mut parent = ctx.root.parent();
            while let Some(dir) = parent {
                if !dir.starts_with(&ctx.repo) {
                    break;
                }
                let package = dir.join("package.json");
                let workspace = if package.exists() {
                    let value: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(package)?)
                            .map_err(|e| config_error(e.to_string()))?;
                    value.get("workspaces").is_some()
                } else {
                    false
                };
                if workspace || dir.join("pnpm-workspace.yaml").exists() {
                    return Err(config_error(
                        "independent Node projects cannot inherit a parent workspace lock; shared workspace lock ownership is unsupported",
                    ));
                }
                parent = dir.parent();
            }
        }

        if ctx.name.is_some() && ctx.root.join("pyproject.toml").exists() {
            let mut parent = ctx.root.parent();
            while let Some(dir) = parent {
                if !dir.starts_with(&ctx.repo) {
                    break;
                }
                let value = read_config(&dir.join("pyproject.toml"))?;
                if value
                    .get("tool")
                    .and_then(|t| t.get("uv"))
                    .and_then(|u| u.get("workspace"))
                    .is_some()
                {
                    return Err(config_error(
                        "independent Python projects cannot inherit a parent uv workspace lock; shared workspace lock ownership is unsupported",
                    ));
                }
                parent = dir.parent();
            }
        }

        if let Some(includes) = &ctx.includes {
            let package_path = ctx.root.strip_prefix(&ctx.repo).expect("contained root");
            if !includes
                .iter()
                .any(|include| package_path.starts_with(include))
            {
                return Err(config_error(
                    "project history paths must include its own package directory",
                ));
            }
        }

        // Reject path escapes before hooks or file mutation, including existing symlinks.
        for entry in &ctx.config.version_files {
            relative(Path::new(&entry.glob))?;
            let pattern = ctx.root.join(&entry.glob).to_string_lossy().into_owned();
            for path in glob::glob(&pattern).map_err(|e| config_error(e.to_string()))? {
                let path = path.map_err(|e| config_error(e.to_string()))?;
                Self::contained(&ctx.root, path.strip_prefix(&ctx.root).unwrap())?;
            }
        }
        for a in &ctx.config.artifacts {
            for file in a.output.iter().chain(a.files.iter()) {
                relative(Path::new(file))?;
                Self::contained(&ctx.root, Path::new(file))?;
            }
        }
        Self::contained(&ctx.root, Path::new("CHANGELOG.md"))?;
        Ok(ctx)
    }

    pub fn contained(root: &Path, relative_path: &Path) -> Result<()> {
        relative(relative_path)?;
        let canonical_root = root.canonicalize()?;
        let mut path = canonical_root.join(relative_path);
        while !path.exists() {
            if std::fs::symlink_metadata(&path).is_ok() {
                return Err(config_error("dangling symlink in project path"));
            }
            if !path.pop() {
                return Err(config_error("invalid project path"));
            }
        }
        if !path.canonicalize()?.starts_with(&canonical_root) {
            return Err(config_error(format!(
                "path escapes project: {}",
                relative_path.display()
            )));
        }
        Ok(())
    }

    pub fn physical_path(path: &Path) -> Result<PathBuf> {
        let mut ancestor = path.to_path_buf();
        let mut suffix = Vec::new();
        while !ancestor.exists() {
            suffix.push(
                ancestor
                    .file_name()
                    .ok_or_else(|| config_error("invalid release path"))?
                    .to_os_string(),
            );
            if !ancestor.pop() {
                return Err(config_error("invalid release path"));
            }
        }
        let mut physical = ancestor.canonicalize()?;
        for part in suffix.into_iter().rev() {
            physical.push(part);
        }
        Ok(physical)
    }

    pub fn tag(&self, version: &Version) -> String {
        format!("{}{version}", self.prefix)
    }
    pub fn tag_version(&self, tag: &str) -> Option<Version> {
        Version::parse(tag.strip_prefix(&self.prefix)?).ok()
    }
    pub fn version(&self, project: &dyn ProjectType) -> Result<Version> {
        if project.is_tag_versioned() {
            let released = self
                .latest_tag(None, false)?
                .and_then(|tag| self.tag_version(&tag))
                .unwrap_or_else(|| Version::new(0, 0, 0));
            let prepared = git::latest_release_marker(&self.repo, &self.prefix)?
                .and_then(|tag| self.tag_version(&tag));
            let mut current = prepared
                .filter(|v| v.cmp_precedence(&released).is_gt())
                .unwrap_or(released);
            // Tag-versioned projects have no manifest to carry an interrupted
            // bump. A dirty, selected changelog section is their on-disk target.
            let changelog_path = self.repo_path(Path::new("CHANGELOG.md"))?;
            if (git::changed_paths(&self.repo)?.contains(&changelog_path)
                || git::untracked_files(&self.repo)?
                    .iter()
                    .any(|p| Path::new(p) == changelog_path))
                && let Ok(content) = std::fs::read_to_string(self.root.join("CHANGELOG.md"))
            {
                let headers =
                    regex::Regex::new(r"(?m)^##[ \t]+\[([^\]]+)\]").expect("valid heading regex");
                if let Some(version) = headers
                    .captures_iter(&content)
                    .find_map(|capture| Version::parse(&capture[1]).ok())
                    && version.cmp_precedence(&current).is_gt()
                {
                    current = version;
                }
            }
            Ok(current)
        } else {
            project.read_version(&self.root)
        }
    }
    pub fn latest_tag(&self, excluded: Option<&str>, strict: bool) -> Result<Option<String>> {
        let mut tags: Vec<_> = git::all_tags(&self.repo)?
            .into_iter()
            .filter_map(|tag| {
                if excluded == Some(tag.as_str()) {
                    None
                } else {
                    self.tag_version(&tag).map(|v| (v, tag))
                }
            })
            .collect();
        tags.sort_by(|a, b| b.0.cmp_precedence(&a.0).then_with(|| a.1.cmp(&b.1)));
        if tags
            .windows(2)
            .any(|p| p[0].0.cmp_precedence(&p[1].0).is_eq())
        {
            return Err(config_error(
                "multiple tags have the same semantic version precedence in selected namespace",
            ));
        }
        if let Some((_, tag)) = tags.first()
            && !git::is_ancestor(&self.repo, &format!("refs/tags/{tag}"), "HEAD")?
        {
            let message = format!(
                "latest project tag {tag} is not reachable from HEAD; fetch and reconcile before releasing"
            );
            if strict {
                return Err(Error::CheckFailed(message));
            }
            eprintln!("Warning: {message}");
            return Ok(tags.into_iter().find_map(|(_, t)| {
                git::is_ancestor(&self.repo, &format!("refs/tags/{t}"), "HEAD")
                    .ok()
                    .filter(|b| *b)
                    .map(|_| t)
            }));
        }
        Ok(tags.into_iter().next().map(|(_, t)| t))
    }
    pub fn commits(&self, tag: Option<&str>, changelog: bool) -> Result<Vec<git::Commit>> {
        let commits = if changelog {
            git::changelog_commits_since_tag(&self.repo, tag)?
        } else {
            git::commits_since_tag(&self.repo, tag)?
        };
        if self.independent_roots.is_empty() {
            return Ok(commits);
        }
        let mut selected = Vec::new();
        for commit in commits {
            let paths = git::commit_paths(&self.repo, &commit.hash)?;
            let relevant = paths.iter().any(|p| match &self.includes {
                Some(includes) => includes.iter().any(|include| p.starts_with(include)),
                None => !self
                    .independent_roots
                    .iter()
                    .any(|root| self.repo.join(p).starts_with(root)),
            });
            if relevant {
                selected.push(commit);
            }
        }
        Ok(selected)
    }
    pub fn config_path(&self) -> PathBuf {
        self.root.join("vership.toml")
    }
    pub fn repo_path(&self, path: &Path) -> Result<PathBuf> {
        Self::contained(&self.root, path)?;
        Ok(self
            .root
            .join(path)
            .strip_prefix(&self.repo)
            .map_err(|e| config_error(e.to_string()))?
            .to_path_buf())
    }
}
