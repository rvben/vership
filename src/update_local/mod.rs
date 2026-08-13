pub mod managers;
pub mod pathscan;
pub mod report;

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::output::OutputConfig;
use crate::verify::checkers;
use crate::verify::targets::{self, Target};

use managers::{Install, Manager, Package, Source};
use pathscan::BinaryReport;

/// What happened to one manager's install of this project's package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The manager already held the target version.
    AlreadyCurrent,
    /// The manager was brought to the target version.
    Updated,
    /// The command that would bring this manager to the target version, not
    /// run: either a dry run, or a pass held back by another manager's registry.
    Planned,
    /// The registry does not serve the target version yet: either the
    /// pre-flight check saw the gap and nothing ran, or the install ran and
    /// could not resolve the version.
    Pending,
    /// Deliberately left alone: reinstalling would fetch different code.
    Skipped,
    /// The install ran and did not leave the target version in place.
    Failed,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::AlreadyCurrent => "already-current",
            Action::Updated => "updated",
            Action::Planned => "planned",
            Action::Pending => "pending",
            Action::Skipped => "skipped",
            Action::Failed => "failed",
        }
    }
}

/// One row of the update report.
#[derive(Debug)]
pub struct InstallReport {
    pub manager: Manager,
    pub package: String,
    pub before: String,
    pub after: Option<String>,
    pub action: Action,
    pub detail: Option<String>,
    pub commands: Vec<Vec<String>>,
}

/// Bring every local install of this project's package to `version`.
pub fn run(
    version: Option<&str>,
    only: Option<&str>,
    skip: Option<&str>,
    dry_run: bool,
    output: &OutputConfig,
) -> Result<()> {
    let root = Path::new(".");
    let config = Config::load(Path::new("vership.toml"));
    let version = crate::verify::resolve_version(root, &config, version)?;
    let selected = select_managers(only, skip)?;

    // Detecting the tap reads the whole project, so it is left undone unless
    // brew is one of the managers this run is about.
    let homebrew = match selected.contains(&Manager::Brew) {
        true => homebrew_target(root, &config)?,
        false => None,
    };
    // Resolved once: these carry the member directories that decide which path
    // installs belong to this project, and the binary names to look for on
    // `$PATH` whether or not any manager turns out to hold them. Left unread
    // when cargo is not one of this run's managers, for the same reason the tap
    // above is: a manifest no selected manager needs cannot fail the run.
    let cargo = match selected.contains(&Manager::Cargo) {
        true => targets::cargo_local_packages(root)?,
        false => Vec::new(),
    };
    let packages = packages(root, homebrew.as_ref(), &selected, &cargo)?;
    let considered = considered(&packages);
    let mut installs: Vec<Install> = Vec::new();
    for (manager, package) in &packages {
        installs.extend(managers::probe_all(*manager, package));
    }

    // Every manager is decided before any of them is touched, so the decision
    // to run nothing is taken with the whole machine in view.
    let agent = checkers::default_agent();
    let registries = Registries::live(&agent, homebrew.as_ref());
    let owned = owned_dirs(root, &cargo);
    let decisions: Vec<Decision> = installs
        .iter()
        .map(|install| decide(&registries, &owned, install, &version))
        .collect();
    let hold = hold(dry_run, &decisions);

    let mut reports: Vec<InstallReport> = Vec::new();
    let mut current: Vec<Install> = Vec::new();
    for (install, decision) in installs.iter().zip(decisions) {
        let (report, after) = carry_out(&registries, install, &version, decision, hold, output);
        current.push(after.unwrap_or_else(|| install.clone()));
        reports.push(report);
    }

    let names = binary_names(&current, &cargo);
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let binaries = pathscan::scan(&dirs, &names, &current, pathscan::is_executable, |path| {
        std::fs::canonicalize(path).ok()
    });

    let verdict = verdict(&reports, &binaries, &version);
    report::render(
        &report::Report {
            version: &version,
            ok: verdict.is_ok(),
            dry_run,
            installs: &reports,
            binaries: &binaries,
            outstanding: &outstanding_managers(&reports),
            considered: &considered,
        },
        output,
    );
    verdict
}

/// The directories whose path installs belong to this project: the project root
/// and every Cargo package in it.
///
/// `cargo install --path .` records the directory holding the crate's manifest,
/// which in a workspace is the member directory rather than the root the
/// command was run from. Comparing against the root alone therefore reads a
/// workspace's own install as some other project's and leaves it behind.
fn owned_dirs(root: &Path, cargo: &[targets::CargoLocalPackage]) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    for package in cargo {
        if !dirs.contains(&package.dir) {
            dirs.push(package.dir.clone());
        }
    }
    dirs
}

/// The package names this run asked each manager about, for the report.
///
/// An empty `installs` has two very different causes: this project publishes
/// nothing a manager could hold, or it does and none is installed. Naming what
/// was asked about makes those distinguishable instead of leaving the caller to
/// read an empty list as either.
fn considered(packages: &[(Manager, Package)]) -> Vec<(Manager, Vec<String>)> {
    packages
        .iter()
        .map(|(manager, package)| {
            let names = match package {
                Package::Named(name) => vec![name.clone()],
                Package::Candidates(names) => names.clone(),
            };
            (*manager, names)
        })
        .collect()
}

/// The managers to probe, and the package name each should be asked about.
/// A manager the project has no package for is not probed at all.
///
/// Only a selected manager's manifest is read, so a broken package.json in a
/// project that also has a Cargo.toml cannot fail a `--managers cargo` run.
fn packages(
    root: &Path,
    homebrew: Option<&(String, Vec<String>)>,
    selected: &[Manager],
    cargo: &[targets::CargoLocalPackage],
) -> Result<Vec<(Manager, Package)>> {
    let mut packages = Vec::new();
    for manager in selected {
        let package = match manager {
            // Every Cargo package in the project, not just a root one: a
            // workspace root declares no package of its own, and its members
            // are what `cargo install` holds. Unpublishable crates are included
            // too, because `cargo install --path` reaches them.
            Manager::Cargo => match cargo {
                [] => None,
                packages => Some(Package::Candidates(
                    packages.iter().map(|p| p.name.clone()).collect(),
                )),
            },
            Manager::Uv => targets::pypi_project_name(root)?.map(Package::Named),
            Manager::Npm => targets::npm_package_name(root)?.map(Package::Named),
            Manager::Brew => homebrew.map(|(_, formulas)| Package::Candidates(formulas.clone())),
        };
        if let Some(package) = package {
            packages.push((*manager, package));
        }
    }
    Ok(packages)
}

/// The tap and formula candidates for this project, or None when it publishes
/// no formula. Detection is `verify`'s, so a repo with no Homebrew target never
/// probes a formula name guessed from its crate name, which could collide with
/// an unrelated formula someone has installed.
fn homebrew_target(root: &Path, config: &Config) -> Result<Option<(String, Vec<String>)>> {
    let remote = crate::git::remote_url(root).unwrap_or(None);
    let tag_only = crate::project::detect(root, config.project.project_type.as_deref())
        .map(|p| p.publishes_only_git_tag())
        .unwrap_or(false);
    let detected =
        targets::detect_targets(root, &config.verify, remote.as_deref(), tag_only, None)?;
    Ok(detected.into_iter().find_map(|target| match target {
        Target::Homebrew { tap, formulas } => Some((tap, formulas)),
        _ => None,
    }))
}

/// What one install needs, decided without changing anything.
#[derive(Debug)]
enum Decision {
    AlreadyCurrent,
    Skip(String),
    /// The registry does not serve the target version yet, with the commands
    /// that will run once it does.
    Wait(String, Vec<Vec<String>>),
    Run(Vec<Vec<String>>),
}

/// Why this pass runs no command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    DryRun,
    /// A registry has not caught up, so no manager is touched.
    Waiting,
}

/// What this run needs to ask a registry a question, fixed for the whole pass.
///
/// The roots are carried rather than reached for, the way every checker already
/// takes its own `base`, so a test can point one at a server it controls and
/// see which document the check actually asks for.
struct Registries<'a> {
    agent: &'a ureq::Agent,
    homebrew: Option<&'a (String, Vec<String>)>,
    crates: &'a str,
    pypi: &'a str,
    npm: &'a str,
    raw_github: &'a str,
}

impl<'a> Registries<'a> {
    fn live(agent: &'a ureq::Agent, homebrew: Option<&'a (String, Vec<String>)>) -> Self {
        Registries {
            agent,
            homebrew,
            crates: checkers::CRATES_IO,
            pypi: checkers::PYPI,
            npm: checkers::NPM,
            raw_github: checkers::RAW_GITHUB,
        }
    }
}

/// Decide what one install needs: what would bring it to the target version,
/// and whether the registry can serve that yet. Nothing here changes anything.
fn decide(
    registries: &Registries<'_>,
    owned: &[PathBuf],
    install: &Install,
    version: &str,
) -> Decision {
    let commands = match plan(install, version, owned) {
        Planned::AlreadyCurrent => return Decision::AlreadyCurrent,
        Planned::Skip(reason) => return Decision::Skip(reason),
        Planned::Install(commands) => commands,
    };
    // Homebrew cannot be told which version to install, so `brew upgrade`
    // against a tap that has not caught up is a silent no-op reported as a
    // success. Asking first is the only way to tell that apart from an update.
    // The pinned forms the other managers use fail loudly instead, and their
    // failure is judged in `attempt_install` rather than here.
    match registries.gap(install, version) {
        Some(detail) => Decision::Wait(detail, commands),
        None => Decision::Run(commands),
    }
}

/// Whether this pass installs anything at all.
///
/// One lagging registry holds back every manager, not just its own. The run
/// then exits `unpublished`, which promises the caller that retrying is the
/// whole remedy, and a half-updated machine is not something a retry can
/// describe. Registries publish at different speeds on the same release, so
/// this is the ordinary case rather than an edge one.
fn hold(dry_run: bool, decisions: &[Decision]) -> Option<Hold> {
    if dry_run {
        return Some(Hold::DryRun);
    }
    decisions
        .iter()
        .any(|d| matches!(d, Decision::Wait(..)))
        .then_some(Hold::Waiting)
}

/// Act on one decision, and re-probe to find out what the manager actually left
/// behind. The manager is the authority on the version it installed; no binary
/// is executed to ask.
fn carry_out(
    registries: &Registries<'_>,
    install: &Install,
    version: &str,
    decision: Decision,
    hold: Option<Hold>,
    output: &OutputConfig,
) -> (InstallReport, Option<Install>) {
    let row = |action: Action, detail: Option<String>, commands: Vec<Vec<String>>| InstallReport {
        manager: install.manager,
        package: install.package.clone(),
        before: install.version.clone(),
        after: None,
        action,
        detail,
        commands,
    };

    let mut commands = match decision {
        Decision::AlreadyCurrent => {
            let mut report = row(Action::AlreadyCurrent, None, Vec::new());
            report.after = Some(install.version.clone());
            return (report, None);
        }
        Decision::Skip(reason) => return (row(Action::Skipped, Some(reason), Vec::new()), None),
        Decision::Wait(detail, commands) => {
            return (row(Action::Pending, Some(detail), commands), None);
        }
        Decision::Run(commands) => commands,
    };

    if let Some(hold) = hold {
        let detail = match hold {
            Hold::DryRun => None,
            Hold::Waiting => Some("held until every registry serves this version".to_string()),
        };
        return (row(Action::Planned, detail, commands), None);
    }

    let run = |argv: &[Vec<String>]| -> std::result::Result<(), String> {
        for argv in argv {
            crate::output::print_step(&argv.join(" "));
            if let Err(e) = managers::execute(argv, output.is_json()) {
                return Err(e.to_string());
            }
        }
        Ok(())
    };
    if let Some((action, detail)) = attempt_install(install, version, &mut commands, run, || {
        registries.gap(install, version)
    }) {
        return (row(action, Some(detail), commands), None);
    }

    match managers::reprobe(install) {
        Some(after) if after.version == version => {
            let mut report = row(Action::Updated, None, commands);
            report.after = Some(after.version.clone());
            (report, Some(after))
        }
        Some(after) => {
            let detail = format!(
                "{} reports {} after installing",
                install.manager.name(),
                after.version
            );
            let mut report = row(Action::Failed, Some(detail), commands);
            report.after = Some(after.version.clone());
            (report, Some(after))
        }
        None => (
            row(
                Action::Failed,
                Some(format!(
                    "{} no longer reports this package",
                    install.manager.name()
                )),
                commands,
            ),
            None,
        ),
    }
}

/// Install one manager's package, and judge the outcome when it does not
/// complete. None means the commands ran; anything else is the row they earned.
///
/// A failed install is not automatically the operator's problem. Every pinned
/// form in `install_commands` fails loudly and changes nothing when the index
/// cannot resolve the version, which describes a release still propagating far
/// more often than a broken machine. Reporting that as a general error tells
/// the caller that retrying cannot help, which is both wrong and the one thing
/// that stops `tarry cmd -- vership update-local` from closing the release on
/// its own.
///
/// So the failure is judged rather than assumed. A manager with a
/// cache-bypassing form gets exactly one more attempt, which separates a stale
/// local index from a registry that has genuinely not served the version yet;
/// then the index the installer resolves against decides. Not finding the
/// version there is `pending`, which exits `unpublished` and retryable. Finding
/// it leaves no reading under which waiting helps, so that stays a failure.
///
/// `run` executes a command list, erroring with the message of the first
/// failure. `gap` describes why the registry cannot serve this version, or None
/// when it can; both are injected so the judgement is testable without a
/// registry.
fn attempt_install(
    install: &Install,
    version: &str,
    commands: &mut Vec<Vec<String>>,
    mut run: impl FnMut(&[Vec<String>]) -> std::result::Result<(), String>,
    gap: impl FnOnce() -> Option<String>,
) -> Option<(Action, String)> {
    let mut detail = match run(commands) {
        Ok(()) => return None,
        Err(detail) => detail,
    };
    if let Some(retry) = managers::retry_commands(install, version) {
        // Reported as part of this install, because it ran as part of it.
        commands.extend(retry.iter().cloned());
        match run(&retry) {
            Ok(()) => return None,
            Err(retried) => detail = retried,
        }
    }
    match gap() {
        Some(gap) => Some((Action::Pending, gap)),
        None => Some((Action::Failed, detail)),
    }
}

impl Registries<'_> {
    /// Whether the registry cannot serve this version yet, described.
    ///
    /// The question is the installer's, not `verify`'s: each manager is asked
    /// about the document its own resolver reads, because a registry that
    /// serves more than one view of itself can have them disagree. PyPI is the
    /// case in point, publishing the JSON API and the simple index as
    /// separately cached documents; only the second decides whether `uv tool
    /// install` resolves, so only the second is worth asking here.
    ///
    /// Any answer other than "the target version is there" means waiting is the
    /// right response, including a network error, so all of them report a gap.
    fn gap(&self, install: &Install, version: &str) -> Option<String> {
        use crate::verify::CheckResult;

        let name = &install.package;
        let result = match install.manager {
            // A path install builds the working tree, which no registry speaks
            // for. Its failures are the operator's, never a pending release.
            Manager::Cargo if matches!(install.source, Source::Path(_)) => return None,
            Manager::Cargo => checkers::crates(self.agent, self.crates, name, version),
            Manager::Uv => checkers::pypi_simple(
                self.agent,
                self.pypi,
                &managers::normalize_pypi(name),
                version,
            ),
            Manager::Npm => checkers::npm(self.agent, self.npm, name, version),
            Manager::Brew => {
                let (tap, formulas) = self.homebrew?;
                checkers::homebrew(self.agent, self.raw_github, tap, formulas, version)
            }
        };
        match result {
            CheckResult::Found(_) => None,
            CheckResult::FoundOld(found) => {
                Some(format!("registry serves {found}, waiting for {version}"))
            }
            CheckResult::NotFound => Some(format!("{version} is not published yet")),
            CheckResult::Error(e) => Some(format!("registry check failed: {e}")),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Planned {
    AlreadyCurrent,
    Install(Vec<Vec<String>>),
    Skip(String),
}

/// Decide what to do with one install, without touching the network or running
/// anything.
pub(crate) fn plan(install: &Install, version: &str, owned: &[PathBuf]) -> Planned {
    if install.version == version {
        return Planned::AlreadyCurrent;
    }
    match &install.source {
        Source::Path(path) if owned.iter().any(|dir| same_dir(path, dir)) => {
            Planned::Install(managers::install_commands(install, version))
        }
        Source::Path(path) => Planned::Skip(format!(
            "installed from {}, not this project",
            path.display()
        )),
        Source::Foreign(source) => Planned::Skip(format!("installed from {source}")),
        Source::Registry => Planned::Install(managers::install_commands(install, version)),
    }
}

/// Whether two paths name the same directory, resolving symlinks where both
/// exist and comparing literally where they do not.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Every executable name to look for on `$PATH`, first-seen order.
///
/// The project's own declared binaries are included alongside the ones the
/// surviving installs provide, so the scan does not depend on a manager having
/// been found. Deriving the list from discovered installs alone makes the
/// shadowing check inherit every gap in probing: with nothing detected there is
/// nothing to look for, and a stale copy winning `$PATH` is reported as a clean
/// pass. That is the failure this list is meant to catch, so it cannot be
/// conditioned on the detection that failed.
fn binary_names(installs: &[Install], cargo: &[targets::CargoLocalPackage]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let declared = cargo.iter().flat_map(|p| p.bins.iter());
    for bin in installs.iter().flat_map(|i| i.bins.iter()).chain(declared) {
        if !names.contains(bin) {
            names.push(bin.clone());
        }
    }
    names
}

/// The exit condition.
///
/// A failed install outranks a lagging registry: both leave the machine off the
/// target version, but only the first needs a human. Shadowing is judged per
/// binary, once no outstanding install can still change what `$PATH` resolves
/// to for it.
fn verdict(reports: &[InstallReport], binaries: &[BinaryReport], version: &str) -> Result<()> {
    let failed: Vec<String> = reports
        .iter()
        .filter(|r| r.action == Action::Failed)
        .map(|r| match &r.detail {
            Some(detail) => format!("{}: {detail}", r.manager.name()),
            None => r.manager.name().to_string(),
        })
        .collect();
    if !failed.is_empty() {
        return Err(Error::Other(format!(
            "update failed: {}",
            failed.join("; ")
        )));
    }

    let pending: Vec<&str> = reports
        .iter()
        .filter(|r| r.action == Action::Pending)
        .map(|r| r.manager.name())
        .collect();
    if !pending.is_empty() {
        return Err(Error::Unpublished(format!(
            "{version} not available yet for: {}",
            pending.join(", ")
        )));
    }

    let outstanding = outstanding_managers(reports);

    for binary in binaries {
        let Some(winner) = binary.winner() else {
            continue;
        };
        if outstanding
            .iter()
            .any(|manager| may_still_change(binary, *manager))
        {
            continue;
        }
        if winner.version.as_deref() == Some(version) {
            continue;
        }
        let name = &binary.name;
        let path = winner.path.display();
        // A version nothing on this machine can read is not evidence of the
        // wrong version, and "not <version>" asserts exactly that. An unmanaged
        // copy is still the failure being reported whatever it holds: it was
        // put there by hand, so no manager moves it on the next release and
        // this check is the only thing that will ever mention it again.
        let message = match (winner.manager, &winner.version) {
            (Some(manager), Some(found)) => format!(
                "{name} on PATH is {path} ({} {found}), not {version}",
                manager.name()
            ),
            (Some(manager), None) => format!(
                "{name} on PATH is {path} ({}), whose version cannot be read to confirm {version}",
                manager.name()
            ),
            (None, _) => {
                format!("{name} on PATH is {path}, an unmanaged copy no manager keeps at {version}")
            }
        };
        return Err(Error::Other(message));
    }
    Ok(())
}

/// Whether an install by `manager`, not yet carried out, could still change
/// which copy of `binary` wins `$PATH`.
///
/// An install rewrites the files its own manager owns. So a manager that owns
/// the winning copy will replace that very file, and one that owns no copy of
/// this binary at all installs somewhere no copy here can place, possibly ahead
/// of the winner. Owning only a shadowed copy is neither: rewriting a file that
/// already loses `$PATH` leaves the winner exactly as it is.
///
/// That last case is the one worth naming, because it reads as a pass. A copy
/// placed by hand in a directory no manager installs into is never what a
/// pending install will fix, so deferring to that install hides a shadow that
/// no run will resolve, and `--dry-run` reports `ok` where the real run reports
/// a failure on the same machine.
pub(super) fn may_still_change(binary: &BinaryReport, manager: Manager) -> bool {
    if binary.winner().and_then(|copy| copy.manager) == Some(manager) {
        return true;
    }
    !binary
        .copies
        .iter()
        .any(|copy| copy.manager == Some(manager))
}

/// The managers whose install has not reached its final state, and so may still
/// change what `$PATH` resolves to. Shared by the exit code and the rendered
/// output so a binary cannot be marked one way and judged the other.
pub(super) fn outstanding_managers(reports: &[InstallReport]) -> Vec<Manager> {
    reports
        .iter()
        .filter(|r| matches!(r.action, Action::Failed | Action::Pending | Action::Planned))
        .map(|r| r.manager)
        .collect()
}

/// Parse `--managers` / `--skip` into the managers to probe.
fn select_managers(only: Option<&str>, skip: Option<&str>) -> Result<Vec<Manager>> {
    let parse = |list: &str| -> Result<Vec<Manager>> {
        list.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                Manager::ALL
                    .into_iter()
                    .find(|m| m.name() == s)
                    .ok_or_else(|| {
                        Error::Config(format!(
                            "unknown manager '{s}': valid managers are {}",
                            Manager::ALL
                                .iter()
                                .map(|m| m.name())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })
            })
            .collect()
    };
    let mut selected = Manager::ALL.to_vec();
    if let Some(only) = only {
        let keep = parse(only)?;
        selected.retain(|m| keep.contains(m));
    }
    if let Some(skip) = skip {
        let drop = parse(skip)?;
        selected.retain(|m| !drop.contains(m));
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pathscan::Copy;

    fn install_at(dir: PathBuf) -> Install {
        install(Manager::Cargo, "0.4.41", Source::Path(dir))
    }

    fn install(manager: Manager, version: &str, source: Source) -> Install {
        Install {
            manager,
            package: "rumdl".to_string(),
            version: version.to_string(),
            source,
            bins: vec!["rumdl".to_string()],
            bin_paths: Vec::new(),
            root: None,
        }
    }

    #[test]
    fn an_install_already_at_the_target_runs_nothing() {
        let here = vec![PathBuf::from(".")];
        let current = install(Manager::Uv, "0.2.48", Source::Registry);
        assert_eq!(plan(&current, "0.2.48", &here), Planned::AlreadyCurrent);
        // The negative control: one patch behind, and a command appears.
        let stale = install(Manager::Uv, "0.2.47", Source::Registry);
        assert!(matches!(plan(&stale, "0.2.48", &here), Planned::Install(_)));
    }

    #[test]
    fn a_path_install_of_this_project_is_rebuilt_from_it() {
        let here = std::env::current_dir().unwrap();
        let install = install(Manager::Cargo, "0.2.29", Source::Path(here.clone()));
        let Planned::Install(commands) = plan(&install, "0.2.48", &[here]) else {
            panic!("a path install of this project must be refreshed");
        };
        assert_eq!(
            commands[0][1..3],
            ["install".to_string(), "--path".to_string()]
        );
    }

    #[test]
    fn a_workspace_member_is_this_project_and_an_unrelated_dir_is_not() {
        // `cargo install --path .` in a workspace records the member directory,
        // not the root the command ran from, so the root alone never matches.
        let root = PathBuf::from("/src/husker");
        let member = root.join("crates/husker");
        let owned = vec![root.clone(), member.clone()];

        let install = install(Manager::Cargo, "0.4.41", Source::Path(member));
        assert!(
            matches!(plan(&install, "0.4.42", &owned), Planned::Install(_)),
            "a member's own install belongs to this project"
        );
        // The negative control, and the reason ownership is a member list
        // rather than a prefix test: a nested checkout sits under the root
        // without being part of this workspace.
        let nested = install_at(root.join("vendor/other"));
        let planned = plan(&nested, "0.4.42", &owned);
        assert!(
            matches!(&planned, Planned::Skip(reason) if reason.contains("vendor/other")),
            "got {planned:?}"
        );
    }

    #[test]
    fn every_cargo_package_directory_is_one_this_project_owns() {
        // The list `plan` judges against. Built from the root alone, a
        // workspace's own install reads as some other project's, so the two
        // halves are checked separately: this is where the member directories
        // enter, and the test above is what does with them.
        let root = PathBuf::from("/src/husker");
        let member = |name: &str, bins: &[&str]| targets::CargoLocalPackage {
            name: name.to_string(),
            dir: root.join("crates").join(name),
            bins: bins.iter().map(|b| b.to_string()).collect(),
        };
        let dirs = owned_dirs(
            &root,
            &[member("husker", &["husker"]), member("husker-core", &[])],
        );
        assert_eq!(
            dirs,
            vec![
                root.clone(),
                root.join("crates/husker"),
                root.join("crates/husker-core"),
            ],
            "a library member counts too: `cargo install --path` reaches it"
        );
    }

    #[test]
    fn a_path_install_of_another_project_is_left_alone() {
        let install = install(
            Manager::Cargo,
            "0.2.29",
            Source::Path(PathBuf::from("/somewhere/else")),
        );
        let planned = plan(&install, "0.2.48", &[PathBuf::from("/here")]);
        assert!(
            matches!(&planned, Planned::Skip(reason) if reason.contains("/somewhere/else")),
            "got {planned:?}"
        );
    }

    #[test]
    fn a_git_install_is_left_alone() {
        let install = install(
            Manager::Cargo,
            "0.2.29",
            Source::Foreign("https://github.com/other/rumdl#abc".to_string()),
        );
        assert!(matches!(
            plan(&install, "0.2.48", &[PathBuf::from(".")]),
            Planned::Skip(_)
        ));
    }

    #[test]
    fn one_lagging_registry_holds_back_every_manager() {
        let ready = || Decision::Run(vec![vec!["cargo".to_string()]]);
        let waiting = || Decision::Wait("not published yet".to_string(), Vec::new());

        assert_eq!(hold(false, &[ready(), waiting()]), Some(Hold::Waiting));
        // The positive control: with nothing waiting, the ready install runs.
        assert_eq!(hold(false, &[ready(), ready()]), None);
        // A dry run holds everything back whatever the registries say.
        assert_eq!(hold(true, &[ready(), ready()]), Some(Hold::DryRun));
        assert_eq!(hold(true, &[waiting()]), Some(Hold::DryRun));
        // Decisions that need no command are not a reason to hold.
        assert_eq!(
            hold(
                false,
                &[Decision::AlreadyCurrent, Decision::Skip("git".to_string())]
            ),
            None
        );
    }

    #[test]
    fn a_held_install_reports_its_command_without_running_it() {
        let install = install(Manager::Cargo, "0.2.29", Source::Registry);
        let commands = vec![vec!["cargo".to_string(), "install".to_string()]];
        let output = OutputConfig::new(crate::cli::OutputFormat::Json, false);
        // A hold returns before anything is run or asked, so nothing here
        // reaches a registry.
        let agent = checkers::default_agent();
        let registries = Registries::live(&agent, None);

        let (report, after) = carry_out(
            &registries,
            &install,
            "0.2.48",
            Decision::Run(commands.clone()),
            Some(Hold::Waiting),
            &output,
        );
        assert_eq!(report.action, Action::Planned);
        assert_eq!(report.commands, commands);
        assert!(report.detail.is_some(), "a hold states why it held");
        assert!(after.is_none(), "nothing ran, so nothing was re-probed");

        // A dry run holds the same way, and says nothing about other managers.
        let (report, _) = carry_out(
            &registries,
            &install,
            "0.2.48",
            Decision::Run(commands.clone()),
            Some(Hold::DryRun),
            &output,
        );
        assert_eq!(report.action, Action::Planned);
        assert_eq!(report.detail, None);
    }

    #[test]
    fn the_uv_pre_flight_reads_the_index_uv_resolves_against() {
        use httpmock::{Method::GET, MockServer};

        // PyPI serves the simple index and the JSON API as separately cached
        // documents, so only the first speaks for whether `uv tool install`
        // will resolve. This server answers both, and disagrees: the JSON API
        // has the version and the simple index does not. Whichever document
        // the check reads decides the verdict, so the verdict names it.
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/simple/my-pkg/");
            then.status(200).json_body(serde_json::json!({
                "meta": {"api-version": "1.1"},
                "name": "my-pkg",
                "versions": ["0.2.47"],
                "files": [],
            }));
        });
        server.mock(|when, then| {
            when.method(GET).path("/pypi/My_Pkg/0.2.48/json");
            then.status(200)
                .json_body(serde_json::json!({"info": {"version": "0.2.48"}}));
        });

        let agent = checkers::default_agent();
        let base = server.base_url();
        let registries = Registries {
            agent: &agent,
            homebrew: None,
            crates: &base,
            pypi: &base,
            npm: &base,
            raw_github: &base,
        };
        // The name is asked for as PEP 503 normalizes it, because that is the
        // only spelling the simple index answers to.
        let mut uv = install(Manager::Uv, "0.2.47", Source::Registry);
        uv.package = "My_Pkg".to_string();
        assert_eq!(
            registries.gap(&uv, "0.2.48"),
            Some("0.2.48 is not published yet".to_string()),
            "the JSON API answer must not be what decides this"
        );
        // The positive control, from the same server: a version the simple
        // index does list leaves no gap.
        assert_eq!(registries.gap(&uv, "0.2.47"), None);
    }

    /// Drive `attempt_install` with a scripted run: each entry is the failure
    /// message for one command list, or None for a list that succeeds.
    fn attempt(
        install: &Install,
        outcomes: &[Option<&str>],
        gap: Option<&str>,
    ) -> (Option<(Action, String)>, Vec<Vec<String>>) {
        let mut commands = managers::install_commands(install, "0.2.48");
        let mut outcomes = outcomes.iter();
        let outcome = attempt_install(
            install,
            "0.2.48",
            &mut commands,
            |_| match outcomes
                .next()
                .expect("attempt_install ran more command lists than the test scripted")
            {
                Some(failure) => Err(failure.to_string()),
                None => Ok(()),
            },
            || gap.map(str::to_string),
        );
        (outcome, commands)
    }

    #[test]
    fn an_install_that_cannot_resolve_the_version_is_pending_not_failed() {
        // The shape of a release still propagating: the install fails, the
        // retry fails, and the index the installer reads does not have it.
        // Calling that a general error tells the caller retrying cannot help.
        let uv = install(Manager::Uv, "0.2.47", Source::Registry);
        let (outcome, commands) = attempt(
            &uv,
            &[Some("exited with 1"), Some("exited with 1")],
            Some("0.2.48 is not published yet"),
        );
        assert_eq!(
            outcome,
            Some((Action::Pending, "0.2.48 is not published yet".to_string()))
        );
        assert_eq!(commands.len(), 2, "the retry ran, so it is reported");
        assert!(
            commands[1].contains(&"--no-cache".to_string()),
            "{commands:?}"
        );
        assert_eq!(
            verdict(&[report(Manager::Uv, Action::Pending)], &[], "0.2.48")
                .unwrap_err()
                .exit_code(),
            8
        );

        // The negative control: the same two failures against an index that
        // does serve the version leave no reading under which waiting helps.
        let (outcome, _) = attempt(
            &uv,
            &[Some("exited with 1"), Some("postinstall crashed")],
            None,
        );
        assert_eq!(
            outcome,
            Some((Action::Failed, "postinstall crashed".to_string())),
            "the last failure is what gets reported"
        );
    }

    #[test]
    fn a_retry_that_succeeds_completes_the_install() {
        let uv = install(Manager::Uv, "0.2.47", Source::Registry);
        // A stale local index: the first attempt cannot resolve, the
        // cache-bypassed one can. Nothing is reported as pending or failed,
        // and the gap check is never consulted.
        let (outcome, commands) = attempt(&uv, &[Some("exited with 1"), None], Some("unused"));
        assert_eq!(outcome, None);
        assert_eq!(commands.len(), 2, "the retry ran, so it is reported");
    }

    #[test]
    fn a_manager_with_no_cache_to_bypass_is_judged_on_its_first_attempt() {
        // cargo gets one attempt, so the scripted run holding a single outcome
        // is itself the assertion: a second call panics.
        let cargo = install(Manager::Cargo, "0.2.47", Source::Registry);
        let (outcome, commands) =
            attempt(&cargo, &[Some("exited with 101")], Some("not published"));
        assert_eq!(
            outcome,
            Some((Action::Pending, "not published".to_string()))
        );
        assert_eq!(
            commands.len(),
            1,
            "nothing was retried, so nothing is added"
        );
    }

    #[test]
    fn a_build_failure_of_this_working_tree_is_never_pending() {
        // A path install has no registry behind it, so `gap` reports none and
        // the failure stays the operator's however many times it is retried.
        let here = std::env::current_dir().unwrap();
        let path = install(Manager::Cargo, "0.2.47", Source::Path(here));
        let (outcome, _) = attempt(&path, &[Some("could not compile rumdl")], None);
        assert_eq!(
            outcome,
            Some((Action::Failed, "could not compile rumdl".to_string()))
        );
    }

    #[test]
    fn an_install_that_completes_first_time_reports_nothing_and_adds_nothing() {
        let uv = install(Manager::Uv, "0.2.47", Source::Registry);
        let (outcome, commands) = attempt(&uv, &[None], Some("unused"));
        assert_eq!(outcome, None);
        assert_eq!(commands, managers::install_commands(&uv, "0.2.48"));
    }

    #[test]
    fn manager_selection_filters_and_rejects_unknown_names() {
        assert_eq!(select_managers(None, None).unwrap(), Manager::ALL.to_vec());
        assert_eq!(
            select_managers(Some("uv,cargo"), None).unwrap(),
            vec![Manager::Cargo, Manager::Uv]
        );
        assert_eq!(
            select_managers(None, Some("brew")).unwrap(),
            vec![Manager::Cargo, Manager::Uv, Manager::Npm]
        );
        assert_eq!(select_managers(Some("uv"), Some("uv")).unwrap(), vec![]);
        let err = select_managers(Some("pipx"), None).unwrap_err().to_string();
        assert!(
            err.contains("pipx") && err.contains("cargo, uv, npm, brew"),
            "{err}"
        );
    }

    fn report(manager: Manager, action: Action) -> InstallReport {
        InstallReport {
            manager,
            package: "rumdl".to_string(),
            before: "0.2.47".to_string(),
            after: None,
            action,
            detail: None,
            commands: Vec::new(),
        }
    }

    fn binary(copies: Vec<(&str, Option<Manager>, Option<&str>)>) -> BinaryReport {
        BinaryReport {
            name: "rumdl".to_string(),
            copies: copies
                .into_iter()
                .map(|(path, manager, version)| Copy {
                    path: PathBuf::from(path),
                    manager,
                    version: version.map(str::to_string),
                })
                .collect(),
        }
    }

    #[test]
    fn nothing_installed_locally_is_a_clean_pass() {
        assert!(verdict(&[], &[], "0.2.48").is_ok());
    }

    #[test]
    fn a_failed_install_outranks_a_lagging_registry() {
        let reports = vec![
            report(Manager::Uv, Action::Pending),
            report(Manager::Cargo, Action::Failed),
        ];
        let err = verdict(&reports, &[], "0.2.48").unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn a_lagging_registry_is_retryable() {
        let reports = vec![report(Manager::Npm, Action::Pending)];
        let err = verdict(&reports, &[], "0.2.48").unwrap_err();
        assert_eq!(err.exit_code(), 8);
    }

    #[test]
    fn a_stale_copy_ahead_of_the_updated_one_fails() {
        let reports = vec![report(Manager::Uv, Action::Updated)];
        let binaries = vec![binary(vec![
            (
                "/home/u/.cargo/bin/rumdl",
                Some(Manager::Cargo),
                Some("0.2.29"),
            ),
            (
                "/home/u/.local/bin/rumdl",
                Some(Manager::Uv),
                Some("0.2.48"),
            ),
        ])];
        let err = verdict(&reports, &binaries, "0.2.48").unwrap_err();
        assert!(
            err.to_string().contains("/home/u/.cargo/bin/rumdl"),
            "{err}"
        );
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn the_same_copies_in_the_other_order_pass() {
        let reports = vec![report(Manager::Uv, Action::Updated)];
        let binaries = vec![binary(vec![
            (
                "/home/u/.local/bin/rumdl",
                Some(Manager::Uv),
                Some("0.2.48"),
            ),
            (
                "/home/u/.cargo/bin/rumdl",
                Some(Manager::Cargo),
                Some("0.2.29"),
            ),
        ])];
        assert!(verdict(&reports, &binaries, "0.2.48").is_ok());
    }

    #[test]
    fn an_unmanaged_copy_that_wins_path_fails() {
        let reports = vec![report(Manager::Uv, Action::Updated)];
        let binaries = vec![binary(vec![
            ("/home/u/mise/bin/rumdl", None, None),
            (
                "/home/u/.local/bin/rumdl",
                Some(Manager::Uv),
                Some("0.2.48"),
            ),
        ])];
        let err = verdict(&reports, &binaries, "0.2.48").unwrap_err();
        assert!(err.to_string().contains("unmanaged"), "{err}");
    }

    #[test]
    fn an_unmanaged_copy_is_never_said_to_be_the_wrong_version() {
        // No manager owns an unmanaged copy, so nothing can report its version
        // without executing it and the report carries None. Saying "not X" of
        // it is an unknowable claim, and a hand-placed copy that happens to be
        // X makes it a false one. It stays a failure either way: no manager
        // will move that file on the next release.
        let reports = vec![report(Manager::Cargo, Action::Updated)];
        let binaries = vec![binary(vec![("/home/u/.local/bin/rumdl", None, None)])];
        let err = verdict(&reports, &binaries, "0.2.48")
            .unwrap_err()
            .to_string();
        assert!(
            !err.contains("not 0.2.48"),
            "an unreadable version must not be asserted to differ: {err}"
        );
        assert!(
            err.contains("unmanaged") && err.contains("/home/u/.local/bin/rumdl"),
            "the copy must still be named as the failure: {err}"
        );

        // The other bound: a version that IS readable and does differ is still
        // stated as the plain fact it is.
        let managed = vec![binary(vec![(
            "/home/u/.cargo/bin/rumdl",
            Some(Manager::Cargo),
            Some("0.2.29"),
        )])];
        let err = verdict(&reports, &managed, "0.2.48")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cargo 0.2.29") && err.contains("not 0.2.48"),
            "a known version that differs is exactly what this may claim: {err}"
        );
    }

    #[test]
    fn shadowing_is_not_judged_while_an_install_is_still_outstanding() {
        // Under --dry-run nothing has been installed, so the stale copy that
        // currently wins PATH is the plan's input, not its verdict.
        let reports = vec![report(Manager::Uv, Action::Planned)];
        let binaries = vec![binary(vec![(
            "/home/u/.cargo/bin/rumdl",
            Some(Manager::Cargo),
            Some("0.2.29"),
        )])];
        assert!(verdict(&reports, &binaries, "0.2.48").is_ok());
    }

    #[test]
    fn an_outstanding_install_defers_only_the_binaries_it_could_change() {
        // A pending cargo install rewrites the copy cargo owns. It cannot move
        // a hand-placed file in a directory cargo never installs into, so the
        // shadow below is already decided and stays decided however the install
        // goes. Deferring it made --dry-run answer `ok` where the real run on
        // the same machine answered with this very failure.
        let reports = vec![report(Manager::Cargo, Action::Planned)];
        let unreachable = vec![binary(vec![
            ("/home/u/.local/bin/rumdl", None, None),
            (
                "/home/u/.cargo/bin/rumdl",
                Some(Manager::Cargo),
                Some("0.2.29"),
            ),
        ])];
        let err = verdict(&reports, &unreachable, "0.2.48")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("/home/u/.local/bin/rumdl"),
            "the copy the pending install cannot reach must be judged now: {err}"
        );

        // The other bound, and the reason this is not simply "always judge":
        // here the pending install owns the copy that wins, so it is about to
        // rewrite the very file whose version is wrong.
        let reachable = vec![binary(vec![(
            "/home/u/.cargo/bin/rumdl",
            Some(Manager::Cargo),
            Some("0.2.29"),
        )])];
        assert!(
            verdict(&reports, &reachable, "0.2.48").is_ok(),
            "a copy the pending install is about to replace must stay unjudged"
        );
    }

    #[test]
    fn an_install_that_reaches_no_path_entry_is_not_a_failure() {
        let reports = vec![report(Manager::Cargo, Action::Updated)];
        let binaries = vec![binary(vec![])];
        assert!(verdict(&reports, &binaries, "0.2.48").is_ok());
    }

    #[test]
    fn binary_names_are_deduplicated_in_first_seen_order() {
        let mut uv = install(Manager::Uv, "1.0.0", Source::Registry);
        uv.bins = vec!["rumdl".to_string()];
        let mut cargo = install(Manager::Cargo, "1.0.0", Source::Registry);
        cargo.bins = vec!["rumdl".to_string(), "rumdl-lsp".to_string()];
        assert_eq!(
            binary_names(&[uv, cargo], &[]),
            vec!["rumdl".to_string(), "rumdl-lsp".to_string()]
        );
    }

    fn declared(name: &str, bins: &[&str]) -> targets::CargoLocalPackage {
        targets::CargoLocalPackage {
            name: name.to_string(),
            dir: PathBuf::from("/src").join(name),
            bins: bins.iter().map(|b| b.to_string()).collect(),
        }
    }

    #[test]
    fn the_projects_own_binaries_are_scanned_even_with_nothing_installed() {
        // The case this exists for: probing found no manager at all, which is
        // exactly when a stale copy on PATH goes unnoticed. Deriving the scan
        // list from installs alone would return nothing to look for here.
        assert_eq!(
            binary_names(&[], &[declared("husker", &["husker"])]),
            vec!["husker".to_string()]
        );

        // A declared binary the installs do not mention is still scanned, and
        // one they both name appears once.
        let mut uv = install(Manager::Uv, "1.0.0", Source::Registry);
        uv.bins = vec!["rumdl".to_string()];
        assert_eq!(
            binary_names(&[uv], &[declared("rumdl", &["rumdl", "rumdl-lsp"])]),
            vec!["rumdl".to_string(), "rumdl-lsp".to_string()]
        );
    }
}
