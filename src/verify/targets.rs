use std::path::{Path, PathBuf};

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
pub(crate) fn formula_candidates(
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
    workspace: Option<CargoWorkspace>,
    bin: Option<Vec<CargoBin>>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: Option<String>,
    publish: Option<toml::Value>,
    autobins: Option<bool>,
}

#[derive(Deserialize)]
struct CargoWorkspace {
    members: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct CargoBin {
    name: Option<String>,
    path: Option<String>,
    #[serde(rename = "required-features")]
    required_features: Option<Vec<String>>,
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

/// A Cargo package's identity: its name, and whether it may reach crates.io.
pub(crate) struct CargoIdentity {
    pub name: String,
    pub publishable: bool,
}

/// Read the package name from Cargo.toml. `publish = false` opts out of
/// publication entirely; a registry list restricts publication to the named
/// registries, so crates.io is in play only when the list names it
/// ("crates-io"). The name is returned either way: a crate that never reaches
/// crates.io can still be installed locally from its path.
pub(crate) fn cargo_identity(root: &Path) -> Result<Option<CargoIdentity>> {
    let path = root.join("Cargo.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let manifest: CargoManifest =
        toml::from_str(&content).map_err(|e| Error::Config(format!("parse Cargo.toml: {e}")))?;
    let Some(package) = manifest.package else {
        return Ok(None);
    };
    let Some(name) = package.name else {
        return Ok(None);
    };
    let publishable = match &package.publish {
        None => true,
        Some(toml::Value::Boolean(b)) => *b,
        Some(toml::Value::Array(registries)) => {
            registries.iter().any(|r| r.as_str() == Some("crates-io"))
        }
        Some(_) => true,
    };
    Ok(Some(CargoIdentity { name, publishable }))
}

/// One Cargo package belonging to this project, and where its manifest lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoLocalPackage {
    pub name: String,
    /// The directory `cargo install --path` records for this package. For a
    /// workspace member that is the member directory, not the workspace root.
    pub dir: PathBuf,
    pub bins: Vec<String>,
}

/// Every Cargo package this project could have a local install of.
///
/// A single-crate repo has exactly one, its root package. A workspace root
/// carries no `[package]` of its own, so its packages are its members; reading
/// only the root manifest makes such a project look like it has no Cargo
/// package at all, and `update-local` then reports "not installed locally" for
/// a binary it installed itself. A repo can be both, a root package that also
/// declares members, so neither branch excludes the other.
///
/// Members are resolved the way cargo resolves them, globs and `exclude`
/// included, so a crate cargo does not consider part of this workspace is not
/// claimed as this project's either.
pub(crate) fn cargo_local_packages(root: &Path) -> Result<Vec<CargoLocalPackage>> {
    let Some(manifest) = read_cargo_manifest(root)? else {
        return Ok(Vec::new());
    };
    let mut packages = Vec::new();
    if let Some(package) = local_package(root, &manifest) {
        packages.push(package);
    }

    let Some(workspace) = &manifest.workspace else {
        return Ok(packages);
    };
    let excluded = member_dirs(root, workspace.exclude.as_deref().unwrap_or(&[]));
    for dir in member_dirs(root, workspace.members.as_deref().unwrap_or(&[])) {
        if excluded.contains(&dir) || packages.iter().any(|p: &CargoLocalPackage| p.dir == dir) {
            continue;
        }
        let Some(member) = read_cargo_manifest(&dir)? else {
            continue;
        };
        if let Some(package) = local_package(&dir, &member) {
            packages.push(package);
        }
    }
    Ok(packages)
}

/// Expand `members`/`exclude` patterns into directories that exist. A pattern
/// that matches nothing contributes nothing, which is how cargo treats a stale
/// entry too.
fn member_dirs(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for pattern in patterns {
        let joined = root.join(pattern);
        let Some(joined) = joined.to_str() else {
            continue;
        };
        let Ok(matches) = glob::glob(joined) else {
            continue;
        };
        for dir in matches.flatten().filter(|p| p.is_dir()) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

fn read_cargo_manifest(dir: &Path) -> Result<Option<CargoManifest>> {
    let path = dir.join("Cargo.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let manifest = toml::from_str(&content)
        .map_err(|e| Error::Config(format!("parse {}: {e}", path.display())))?;
    Ok(Some(manifest))
}

/// The package a manifest declares, with the executables it installs.
///
/// Cargo's own rules: every `[[bin]]` names a binary, and on top of those it
/// infers one named after the package from `src/main.rs` and one per entry in
/// `src/bin`. Inference is per source file and yields to the explicit list, so a
/// `[[bin]]` claiming `src/main.rs` under some other name is the only binary
/// that file produces. `tasmota-cli` is the case in point: it builds `tasmota`
/// and nothing called `tasmota-cli`.
///
/// A library-only crate declares none, and saying otherwise is not harmless:
/// these names are what gets looked for on `$PATH`, so a name that does not
/// exist is reported as a binary missing from it.
///
/// A binary behind `required-features` is left out, because a plain `cargo
/// install` does not build it and so never puts it on `$PATH`. The two mistakes
/// are not equally cheap: leaving one out costs a missed shadow of a binary the
/// release does not ship, while including one turns every stale file of that
/// name into a failed release check. rumdl gates five benchmark binaries this
/// way and installs only `rumdl`.
fn local_package(dir: &Path, manifest: &CargoManifest) -> Option<CargoLocalPackage> {
    let package = manifest.package.as_ref()?;
    let name = package.name.clone()?;
    let explicit: Vec<&CargoBin> = manifest
        .bin
        .iter()
        .flatten()
        .filter(|b| b.required_features.as_deref().unwrap_or(&[]).is_empty())
        .collect();
    let mut bins: Vec<String> = explicit.iter().filter_map(|b| b.name.clone()).collect();

    // Only source files no `[[bin]]` speaks for are inferred from, and only a
    // `path` has to say so. Cargo locates a pathless entry by its name, which
    // is the name inference gives that same file, so the duplicate it would
    // produce is already suppressed by name below.
    let claimed: Vec<PathBuf> = manifest
        .bin
        .iter()
        .flatten()
        .filter_map(|b| b.path.as_ref())
        .map(|path| dir.join(path))
        .collect();
    let mut infer = |source: PathBuf, bin: String| {
        if !claimed.contains(&source) && !bins.contains(&bin) {
            bins.push(bin);
        }
    };
    // `autobins = false` turns inference off outright.
    if package.autobins.unwrap_or(true) {
        let main = dir.join("src/main.rs");
        if main.exists() {
            infer(main, name.clone());
        }
        let mut auto: Vec<(PathBuf, String)> = std::fs::read_dir(dir.join("src/bin"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| auto_bin(&entry.path()))
            .collect();
        // Directory order is not stable across filesystems, and this list
        // reaches the report.
        auto.sort();
        for (source, bin) in auto {
            infer(source, bin);
        }
    }

    Some(CargoLocalPackage {
        name,
        dir: dir.to_path_buf(),
        bins,
    })
}

/// The source file and binary name cargo infers from an entry in `src/bin`: a
/// `*.rs` file, or a directory holding its own `main.rs`.
fn auto_bin(path: &Path) -> Option<(PathBuf, String)> {
    let name = path.file_stem()?.to_str()?.to_string();
    if path.is_dir() {
        let main = path.join("main.rs");
        return main.exists().then_some((main, name));
    }
    (path.extension()? == "rs").then_some((path.to_path_buf(), name))
}

/// Read the distribution name from pyproject.toml.
pub(crate) fn pypi_project_name(root: &Path) -> Result<Option<String>> {
    let path = root.join("pyproject.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let pyproject: Pyproject = toml::from_str(&content)
        .map_err(|e| Error::Config(format!("parse pyproject.toml: {e}")))?;
    Ok(pyproject.project.and_then(|p| p.name))
}

/// Read the package name from package.json. A private package is never
/// published and never installed globally from a registry, so it reads as no
/// name at all.
pub(crate) fn npm_package_name(root: &Path) -> Result<Option<String>> {
    let path = root.join("package.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let package: PackageJson = serde_json::from_str(&content)
        .map_err(|e| Error::Config(format!("parse package.json: {e}")))?;
    if package.private == Some(true) {
        return Ok(None);
    }
    Ok(package.name)
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

    // The crate name is captured even when the crate is unpublishable: it is the
    // Homebrew formula default, since the formula is named after the installed
    // binary rather than the repo.
    let cargo = cargo_identity(root)?;
    let crate_name = cargo.as_ref().map(|c| c.name.clone());
    if let Some(cargo) = cargo
        && cargo.publishable
    {
        targets.push(Target::Crates { name: cargo.name });
    }

    if let Some(name) = pypi_project_name(root)? {
        targets.push(Target::Pypi { name });
    }

    if let Some(name) = npm_package_name(root)? {
        targets.push(Target::Npm { name });
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

    /// Write a manifest at `dir/Cargo.toml`, creating the directory.
    fn manifest(root: &Path, dir: &str, body: &str) {
        let dir = root.join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), body).unwrap();
    }

    fn names(packages: &[CargoLocalPackage]) -> Vec<&str> {
        packages.iter().map(|p| p.name.as_str()).collect()
    }

    /// Create `dir/src/<rel>` with placeholder contents.
    fn source(root: &Path, dir: &str, rel: &str) {
        let path = root.join(dir).join("src").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "fn main() {}\n").unwrap();
    }

    #[test]
    fn a_single_crate_project_yields_its_root_package() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), ".", "[package]\nname = \"rumdl\"\n");
        source(tmp.path(), ".", "main.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(names(&packages), vec!["rumdl"]);
        assert_eq!(packages[0].dir, tmp.path());
        assert_eq!(
            packages[0].bins,
            vec!["rumdl".to_string()],
            "src/main.rs gives one binary named after the package"
        );
    }

    #[test]
    fn a_library_only_crate_declares_no_binary() {
        // These names are what gets looked for on PATH. Defaulting to the
        // package name would report every library crate in a workspace as a
        // binary missing from PATH.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), ".", "[package]\nname = \"husker-core\"\n");
        source(tmp.path(), ".", "lib.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(names(&packages), vec!["husker-core"]);
        assert!(packages[0].bins.is_empty(), "got {:?}", packages[0].bins);
    }

    #[test]
    fn extra_binaries_under_src_bin_are_found() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), ".", "[package]\nname = \"tool\"\n");
        source(tmp.path(), ".", "main.rs");
        source(tmp.path(), ".", "bin/helper.rs");
        source(tmp.path(), ".", "bin/nested/main.rs");
        // Not a binary: no extension cargo builds, and no main.rs beside it.
        source(tmp.path(), ".", "bin/notes.txt");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec![
                "tool".to_string(),
                "helper".to_string(),
                "nested".to_string()
            ]
        );
    }

    #[test]
    fn a_virtual_workspace_yields_its_members() {
        // The husker shape, and the bug this fixes: the root manifest declares
        // no package at all, so reading it alone reports a project with no
        // Cargo package and `update-local` silently does nothing.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), ".", "[workspace]\nmembers = [\"crates/*\"]\n");
        manifest(
            tmp.path(),
            "crates/husker",
            "[package]\nname = \"husker\"\n",
        );
        manifest(
            tmp.path(),
            "crates/husker-core",
            "[package]\nname = \"husker-core\"\n",
        );

        let packages = cargo_local_packages(tmp.path()).unwrap();
        let mut found = names(&packages);
        found.sort();
        assert_eq!(found, vec!["husker", "husker-core"]);

        // The member directory, not the workspace root: that is what `cargo
        // install --path` records and what ownership is decided against.
        let husker = packages.iter().find(|p| p.name == "husker").unwrap();
        assert_eq!(husker.dir, tmp.path().join("crates/husker"));

        // The negative control, against the same fixture: without member
        // resolution the root manifest names nothing.
        assert!(
            cargo_identity(tmp.path()).unwrap().is_none(),
            "the root of a virtual workspace has no package of its own"
        );
    }

    #[test]
    fn a_root_package_that_also_has_members_yields_both() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"top\"\n\n[workspace]\nmembers = [\"sub\"]\n",
        );
        manifest(tmp.path(), "sub", "[package]\nname = \"sub\"\n");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        let mut found = names(&packages);
        found.sort();
        assert_eq!(found, vec!["sub", "top"]);
    }

    #[test]
    fn an_excluded_directory_is_not_this_projects_package() {
        // cargo does not consider an excluded crate part of the workspace, so
        // neither does this: claiming it would let `update-local` reinstall a
        // crate that belongs to some other project.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/vendored\"]\n",
        );
        manifest(tmp.path(), "crates/mine", "[package]\nname = \"mine\"\n");
        manifest(
            tmp.path(),
            "crates/vendored",
            "[package]\nname = \"vendored\"\n",
        );
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(names(&packages), vec!["mine"]);
    }

    #[test]
    fn an_explicit_bin_list_is_what_gets_scanned() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"tool\"\n\n[[bin]]\nname = \"tl\"\n\n[[bin]]\nname = \"tool-lsp\"\n",
        );
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["tl".to_string(), "tool-lsp".to_string()],
            "an explicit [[bin]] list replaces the package-name default"
        );
    }

    #[test]
    fn a_renamed_binary_does_not_also_answer_to_its_package_name() {
        // tasmota-cli's shape: one `[[bin]]` claiming src/main.rs under another
        // name. Inferring the package name from that same file as well invents
        // a `tasmota-cli` executable, which is then reported missing from PATH
        // for the rest of the project's life.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"tasmota-cli\"\n\n[[bin]]\nname = \"tasmota\"\npath = \"src/main.rs\"\n",
        );
        source(tmp.path(), ".", "main.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(packages[0].bins, vec!["tasmota".to_string()]);
    }

    #[test]
    fn two_binaries_built_from_one_source_are_both_scanned() {
        // shelly-cli's shape, and the control for the test above: the same file
        // twice under two names really does install two executables, so
        // suppressing inference must not suppress the explicit list with it.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"shelly-cli\"\n\n[[bin]]\nname = \"shelly\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"shelly-cli\"\npath = \"src/main.rs\"\n",
        );
        source(tmp.path(), ".", "main.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["shelly".to_string(), "shelly-cli".to_string()]
        );
    }

    #[test]
    fn a_binary_declared_and_inferred_is_listed_once() {
        // `[[bin]] name = "helper"` with no path and a src/bin/helper.rs are
        // the same executable, reached explicitly and by inference. Listing it
        // twice would scan PATH for it twice and report it twice.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"tool\"\n\n[[bin]]\nname = \"helper\"\n",
        );
        source(tmp.path(), ".", "main.rs");
        source(tmp.path(), ".", "bin/helper.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["helper".to_string(), "tool".to_string()],
            "helper once, and src/main.rs still gives `tool`"
        );
    }

    #[test]
    fn a_feature_gated_binary_is_not_expected_on_path() {
        // rumdl's shape: benchmark binaries a plain `cargo install` never
        // builds. Looking for them finds whatever old file happens to carry the
        // name, and reports the release as shadowed by it.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"rumdl\"\n\n[[bin]]\nname = \"rumdl\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"benchmark\"\npath = \"benchmark/bin/benchmark.rs\"\nrequired-features = [\"build-benchmarks\"]\n",
        );
        source(tmp.path(), ".", "main.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["rumdl".to_string()],
            "only what `cargo install` puts on PATH"
        );
    }

    #[test]
    fn an_ungated_binary_is_still_expected_on_path() {
        // The control: the same manifest without the gate declares both, so the
        // test above is the gate working rather than a second [[bin]] being
        // dropped.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"rumdl\"\n\n[[bin]]\nname = \"rumdl\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"benchmark\"\npath = \"benchmark/bin/benchmark.rs\"\n",
        );
        source(tmp.path(), ".", "main.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["rumdl".to_string(), "benchmark".to_string()]
        );
    }

    #[test]
    fn autobins_false_leaves_only_the_declared_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            ".",
            "[package]\nname = \"tool\"\nautobins = false\n\n[[bin]]\nname = \"tl\"\npath = \"src/main.rs\"\n",
        );
        source(tmp.path(), ".", "main.rs");
        source(tmp.path(), ".", "bin/helper.rs");
        let packages = cargo_local_packages(tmp.path()).unwrap();
        assert_eq!(
            packages[0].bins,
            vec!["tl".to_string()],
            "src/bin/helper.rs is not inferred when inference is off"
        );
    }

    #[test]
    fn a_project_with_no_cargo_manifest_yields_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(cargo_local_packages(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn a_member_pattern_matching_nothing_is_not_an_error() {
        // A stale members entry is how a repo mid-refactor looks; cargo ignores
        // it, and a hard error here would fail `update-local` for the whole
        // project over a directory nobody installs from.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), ".", "[workspace]\nmembers = [\"gone/*\"]\n");
        assert!(cargo_local_packages(tmp.path()).unwrap().is_empty());
    }
}
