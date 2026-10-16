pub mod checkers;
#[cfg(test)]
pub(crate) mod fixture;
pub mod report;
pub mod targets;

use std::path::PathBuf;

use semver::Version;

use crate::context::ReleaseContext;
use crate::error::{Error, Result};
use crate::output::OutputConfig;

/// Run post-release verification for `version` (defaults to the on-disk version).
pub fn run(
    version: Option<&str>,
    only: Option<&str>,
    skip: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    run_in(&ReleaseContext::resolve(None)?, version, only, skip, output)
}

/// What one verification run checks, resolved without touching the network.
#[derive(Debug)]
pub(crate) struct Plan {
    pub version: String,
    pub tag: String,
    pub targets: Vec<targets::Target>,
}

/// Resolve the version, tag and targets `run_in` checks for the selected
/// project. Manifests are read from `ctx.root`, the remote from `ctx.repo`.
pub(crate) fn plan(
    ctx: &ReleaseContext,
    version: Option<&str>,
    only: Option<&str>,
    skip: Option<&str>,
) -> Result<Plan> {
    let parsed = resolve_version_in(ctx, version)?;
    let version = parsed.to_string();
    let remote = crate::git::remote_url(&ctx.repo)?;
    let resolved = project_targets(ctx, remote.as_deref(), Some(&version))?;
    // A declared list is the project's whole contract, so a request outside it
    // is a mistake to report; detected targets keep their lenient filter.
    let targets = match ctx.verify_targets {
        Some(_) => targets::narrow_targets(resolved, only, skip)?,
        None => targets::filter_targets(resolved, only, skip)?,
    };
    if targets.is_empty() {
        return Err(Error::Config(
            "no verify targets detected or selected".to_string(),
        ));
    }
    Ok(Plan {
        tag: ctx.tag(&parsed),
        version,
        targets,
    })
}

/// The targets the selected project publishes to.
///
/// A declared `verify.targets` list is taken literally; a named project must
/// declare one, so nothing about it is inferred from the repository. Without
/// one, targets are detected from the project root as they always were, minus
/// anything inside a registered independent project.
pub(crate) fn project_targets(
    ctx: &ReleaseContext,
    remote: Option<&str>,
    version: Option<&str>,
) -> Result<Vec<targets::Target>> {
    let config = &ctx.config.verify;
    if let Some(declared) = &ctx.verify_targets {
        return targets::declared_targets(&ctx.root, config, remote, declared, ctx.name.is_some());
    }
    if ctx.name.is_some() {
        return Err(Error::Config(
            "named projects must explicitly declare nonempty verify.targets".to_string(),
        ));
    }
    // Best-effort: a project type whose only published artifact is the git tag
    // (e.g. an Ansible collection) verifies tag-only. Detection failures fall
    // back to the default (full target detection).
    let tag_only = crate::project::detect(&ctx.root, ctx.config.project.project_type.as_deref())
        .map(|p| p.publishes_only_git_tag())
        .unwrap_or(false);
    targets::detect_targets_excluding(
        &ctx.root,
        config,
        remote,
        tag_only,
        version,
        &independent_below(ctx),
    )
}

/// The roots of registered independent projects below the selected root. Their
/// packages are released on their own tags, so the selected project neither
/// verifies nor installs them, whatever version they carry.
pub(crate) fn independent_below(ctx: &ReleaseContext) -> Vec<PathBuf> {
    ctx.independent_roots
        .iter()
        .filter(|root| **root != ctx.root && root.starts_with(&ctx.root))
        .cloned()
        .collect()
}

/// Run post-release verification for the selected project.
pub(crate) fn run_in(
    ctx: &ReleaseContext,
    version: Option<&str>,
    only: Option<&str>,
    skip: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    let Plan {
        version,
        tag,
        targets,
    } = plan(ctx, version, only, skip)?;
    let repo = ctx.repo.as_path();

    let agent = checkers::default_agent();
    let reports: Vec<TargetReport> = targets
        .iter()
        .map(|target| {
            let result = match target {
                targets::Target::Tag => match crate::git::remote_tag_exists(repo, &tag) {
                    Ok(true) => CheckResult::Found(tag.clone()),
                    Ok(false) => CheckResult::NotFound,
                    Err(e) => CheckResult::Error(e.to_string()),
                },
                targets::Target::Release => checkers::release(repo, &tag, &version),
                targets::Target::Crates { name } => {
                    checkers::crates(&agent, checkers::CRATES_IO, name, &version)
                }
                targets::Target::Pypi { name } => {
                    checkers::pypi(&agent, checkers::PYPI, name, &version)
                }
                targets::Target::Npm { name } => {
                    checkers::npm(&agent, checkers::NPM, name, &version)
                }
                targets::Target::Homebrew { tap, formulas } => {
                    checkers::homebrew(&agent, checkers::RAW_GITHUB, tap, formulas, &version)
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

/// The version to verify for the selected project.
///
/// An explicit version is a full semantic version, written bare, with the
/// project's tag prefix, or with the legacy `v`. Otherwise the project's own
/// version (its manifest, or its tag namespace for a tag-versioned type),
/// falling back to the latest tag in the project's namespace.
pub(crate) fn resolve_version_in(ctx: &ReleaseContext, explicit: Option<&str>) -> Result<Version> {
    if let Some(text) = explicit {
        let bare = text
            .strip_prefix(ctx.prefix.as_str())
            .or_else(|| text.strip_prefix('v'))
            .unwrap_or(text);
        return Version::parse(bare)
            .map_err(|e| Error::Version(format!("invalid version '{text}': {e}")));
    }
    let project_type = ctx.config.project.project_type.as_deref();
    if let Ok(project) = crate::project::detect(&ctx.root, project_type)
        && let Ok(version) = ctx.version(project.as_ref())
    {
        return Ok(version);
    }
    if let Some(version) = ctx
        .latest_tag(None, false)?
        .and_then(|tag| ctx.tag_version(&tag))
    {
        return Ok(version);
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

#[cfg(test)]
mod tests {
    use super::fixture::{context, monorepo, write};
    use super::targets::Target;
    use super::*;

    fn pypi_names(targets: &[Target]) -> Vec<&str> {
        targets
            .iter()
            .filter_map(|t| match t {
                Target::Pypi { name } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    fn error(result: Result<Plan>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn the_root_does_not_verify_a_registered_project_at_its_own_version() {
        // mono-py is pinned to 1.0.0, exactly the root release, and sits where
        // the packaging scan looks. Version equality is what claims a pinned
        // companion, so only registration can keep it out.
        let repo = monorepo();
        let plan = plan(&context(repo.path(), None), Some("1.0.0"), None, None).unwrap();
        assert_eq!(pypi_names(&plan.targets), vec!["mono", "mono-sdk"]);
        assert_eq!(plan.tag, "v1.0.0");

        // The negative control: the same tree scanned without the registry
        // claims mono-py, so the exclusion above is what removed it.
        let unregistered = targets::detect_targets(
            &context(repo.path(), None).root,
            &Default::default(),
            Some("https://github.com/acme/mono"),
            false,
            Some("1.0.0"),
        )
        .unwrap();
        assert_eq!(
            pypi_names(&unregistered),
            vec!["mono", "mono-py", "mono-sdk"]
        );
    }

    #[test]
    fn a_named_project_verifies_exactly_its_declared_targets() {
        // The root workflow publishes to PyPI, ghcr and Homebrew; none of that
        // is the cli project's, and none of it may be inferred for it.
        let repo = monorepo();
        let plan = plan(&context(repo.path(), Some("cli")), None, None, None).unwrap();
        assert_eq!(
            plan.targets,
            vec![
                Target::Tag,
                Target::Crates {
                    name: "mono-cli".to_string()
                }
            ]
        );
        assert_eq!(plan.version, "0.3.0", "the project's own manifest version");
        assert_eq!(plan.tag, "cli-v0.3.0");

        let py = plan_for(&repo, "py", None);
        assert_eq!(
            py.targets,
            vec![Target::Pypi {
                name: "mono-py".to_string()
            }]
        );
        assert_eq!(py.tag, "py-v1.0.0");
    }

    fn plan_for(repo: &tempfile::TempDir, project: &str, version: Option<&str>) -> Plan {
        plan(&context(repo.path(), Some(project)), version, None, None).unwrap()
    }

    #[test]
    fn an_explicit_version_is_a_full_semver_in_the_projects_namespace() {
        let repo = monorepo();
        for written in ["0.4.0", "cli-v0.4.0", "v0.4.0"] {
            let plan = plan_for(&repo, "cli", Some(written));
            assert_eq!(
                (plan.version.as_str(), plan.tag.as_str()),
                ("0.4.0", "cli-v0.4.0"),
                "{written}"
            );
        }
        let ctx = context(repo.path(), Some("cli"));
        for rejected in ["0.4", "py-v0.4.0", "cli-0.4.0"] {
            let err = error(plan(&ctx, Some(rejected), None, None));
            assert!(err.contains(rejected), "{rejected}: {err}");
        }
    }

    #[test]
    fn targets_can_only_narrow_a_declared_list() {
        let repo = monorepo();
        let ctx = context(repo.path(), Some("cli"));
        let narrowed = plan(&ctx, None, Some("crates"), None).unwrap();
        assert_eq!(narrowed.targets.len(), 1);
        assert_eq!(narrowed.targets[0].name(), "crates");

        // pypi is a real target name, just not this project's: asking for it
        // must not read as a pass over nothing.
        let err = error(plan(&ctx, None, Some("crates,pypi"), None));
        assert!(
            err.contains("'pypi'") && err.contains("tag, crates"),
            "{err}"
        );
        let err = error(plan(&ctx, None, None, Some("crate")));
        assert!(err.contains("unknown target 'crate'"), "{err}");

        // The root keeps its lenient filter over detected targets.
        let root = context(repo.path(), None);
        let only_npm = plan(&root, Some("1.0.0"), Some("npm"), None);
        assert!(error(only_npm).contains("no verify targets"));
    }

    #[test]
    fn a_named_project_must_configure_identities_the_monorepo_cannot_supply() {
        // The repository is named mono; a formula or image defaulted from that
        // names some other artifact than this project's.
        let repo = monorepo();
        let cli = repo.path().join("packages/cli");
        let declare = |verify: &str| {
            write(&cli, "vership.toml", verify);
            plan(&context(repo.path(), Some("cli")), None, None, None)
        };
        let homebrew = "[verify]\ntargets = [\"homebrew\"]\n";
        assert!(error(declare(homebrew)).contains("verify.tap"));
        let tap_only = "[verify]\ntargets = [\"homebrew\"]\ntap = \"acme/homebrew-tap\"\n";
        assert!(error(declare(tap_only)).contains("verify.formula"));
        assert!(error(declare("[verify]\ntargets = [\"ghcr\"]\n")).contains("verify.image"));

        let configured = "[verify]\ntargets = [\"homebrew\", \"ghcr\"]\n\
                          tap = \"acme/homebrew-tap\"\nformula = \"mono-cli\"\nimage = \"acme/cli\"\n";
        assert_eq!(
            declare(configured).unwrap().targets,
            vec![
                Target::Homebrew {
                    tap: "acme/homebrew-tap".to_string(),
                    formulas: vec!["mono-cli".to_string()]
                },
                Target::Ghcr {
                    image: "acme/cli".to_string()
                }
            ]
        );
    }

    #[test]
    fn a_declared_list_must_be_valid_and_resolvable() {
        let repo = monorepo();
        let cli = repo.path().join("packages/cli");
        let declare = |verify: &str| {
            write(&cli, "vership.toml", verify);
            error(
                ReleaseContext::at(repo.path(), Some("cli"))
                    .and_then(|ctx| plan(&ctx, None, None, None)),
            )
        };
        let duplicate = declare("[verify]\ntargets = [\"tag\", \"tag\"]\n");
        assert!(duplicate.contains("duplicate target 'tag'"), "{duplicate}");
        let unknown = declare("[verify]\ntargets = [\"pip\"]\n");
        assert!(unknown.contains("unknown target 'pip'"), "{unknown}");
        let bad_skip = declare("[verify]\ntargets = [\"tag\"]\nskip = [\"tags\"]\n");
        assert!(bad_skip.contains("verify.skip"), "{bad_skip}");
        // A declared registry the manifest cannot back is a mistake, not an
        // empty check.
        let no_npm = declare("[verify]\ntargets = [\"tag\", \"npm\"]\n");
        assert!(no_npm.contains("'npm'"), "{no_npm}");
        write(
            &cli,
            "Cargo.toml",
            "[package]\nname = \"mono-cli\"\nversion = \"0.3.0\"\npublish = false\n\n[workspace]\n",
        );
        let unpublished = declare("[verify]\ntargets = [\"crates\"]\n");
        assert!(
            unpublished.contains("not published to crates.io"),
            "{unpublished}"
        );
    }
}
