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
    /// The registry does not serve the target version yet, so nothing ran.
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
    let packages = packages(root, homebrew.as_ref(), &selected)?;
    let mut installs: Vec<(Package, Install)> = Vec::new();
    for (manager, package) in packages {
        if let Some(install) = managers::probe(manager, &package) {
            installs.push((package, install));
        }
    }

    // Every manager is decided before any of them is touched, so the decision
    // to run nothing is taken with the whole machine in view.
    let agent = checkers::default_agent();
    let decisions: Vec<Decision> = installs
        .iter()
        .map(|(_, install)| decide(&agent, root, install, &version, homebrew.as_ref()))
        .collect();
    let hold = hold(dry_run, &decisions);

    let mut reports: Vec<InstallReport> = Vec::new();
    let mut current: Vec<Install> = Vec::new();
    for ((package, install), decision) in installs.iter().zip(decisions) {
        let (report, after) = carry_out(install, package, &version, decision, hold, output);
        current.push(after.unwrap_or_else(|| install.clone()));
        reports.push(report);
    }

    let names = binary_names(&current);
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let binaries = pathscan::scan(&dirs, &names, &current, pathscan::is_executable, |path| {
        std::fs::canonicalize(path).ok()
    });

    let verdict = verdict(&reports, &binaries, &version);
    report::render(
        &version,
        verdict.is_ok(),
        dry_run,
        &reports,
        &binaries,
        settled(&reports),
        output,
    );
    verdict
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
) -> Result<Vec<(Manager, Package)>> {
    let mut packages = Vec::new();
    for manager in selected {
        let package = match manager {
            // The crate name is read even from an unpublishable crate: `cargo
            // install --path` reaches it.
            Manager::Cargo => targets::cargo_identity(root)?.map(|c| Package::Named(c.name)),
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
    let detected = targets::detect_targets(root, &config.verify, remote.as_deref(), tag_only)?;
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

/// Decide what one install needs: what would bring it to the target version,
/// and whether the registry can serve that yet. Nothing here changes anything.
fn decide(
    agent: &ureq::Agent,
    root: &Path,
    install: &Install,
    version: &str,
    homebrew: Option<&(String, Vec<String>)>,
) -> Decision {
    let commands = match plan(install, version, root) {
        Planned::AlreadyCurrent => return Decision::AlreadyCurrent,
        Planned::Skip(reason) => return Decision::Skip(reason),
        Planned::Install(commands) => commands,
    };
    // A registry that does not serve the target version yet turns an install
    // into a silent downgrade-to-current: `cargo install <crate> --force`
    // reinstalls whatever is published and exits 0.
    match registry_gap(agent, install, version, homebrew) {
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
    install: &Install,
    package: &Package,
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

    let commands = match decision {
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

    for argv in &commands {
        crate::output::print_step(&argv.join(" "));
        if let Err(e) = managers::execute(argv, output.is_json()) {
            return (row(Action::Failed, Some(e.to_string()), commands), None);
        }
    }

    match managers::probe(install.manager, package) {
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

/// Ask the registry the same question `verify` asks, before running anything.
/// Any answer other than "the target version is live" means waiting is the
/// right response, including a network error, so all of them report a gap.
fn registry_gap(
    agent: &ureq::Agent,
    install: &Install,
    version: &str,
    homebrew: Option<&(String, Vec<String>)>,
) -> Option<String> {
    use crate::verify::CheckResult;

    let name = &install.package;
    let result = match install.manager {
        // A path install builds the working tree, which no registry speaks for.
        Manager::Cargo if matches!(install.source, Source::Path(_)) => return None,
        Manager::Cargo => checkers::crates(agent, checkers::CRATES_IO, name, version),
        Manager::Uv => checkers::pypi(agent, checkers::PYPI, name, version),
        Manager::Npm => checkers::npm(agent, checkers::NPM, name, version),
        Manager::Brew => {
            let (tap, formulas) = homebrew?;
            checkers::homebrew(agent, checkers::RAW_GITHUB, tap, formulas, version)
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Planned {
    AlreadyCurrent,
    Install(Vec<Vec<String>>),
    Skip(String),
}

/// Decide what to do with one install, without touching the network or running
/// anything.
pub(crate) fn plan(install: &Install, version: &str, root: &Path) -> Planned {
    if install.version == version {
        return Planned::AlreadyCurrent;
    }
    match &install.source {
        Source::Path(path) if same_dir(path, root) => {
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

/// Every executable name the surviving installs provide, first-seen order.
fn binary_names(installs: &[Install]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for install in installs {
        for bin in &install.bins {
            if !names.contains(bin) {
                names.push(bin.clone());
            }
        }
    }
    names
}

/// The exit condition.
///
/// A failed install outranks a lagging registry: both leave the machine off the
/// target version, but only the first needs a human. Shadowing is judged only
/// once nothing is outstanding, because an install that has not run yet is
/// legitimately not at the target version.
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

    if !settled(reports) {
        return Ok(());
    }

    for binary in binaries {
        let Some(winner) = binary.winner() else {
            continue;
        };
        if winner.version.as_deref() != Some(version) {
            let owner = match (winner.manager, &winner.version) {
                (Some(manager), Some(found)) => format!("{} {found}", manager.name()),
                (Some(manager), None) => manager.name().to_string(),
                (None, _) => "an unmanaged copy".to_string(),
            };
            return Err(Error::Other(format!(
                "{} on PATH is {} ({owner}), not {version}",
                binary.name,
                winner.path.display()
            )));
        }
    }
    Ok(())
}

/// Whether every install has reached its final state, so what `$PATH` resolves
/// to now is this run's outcome rather than its input. An outstanding install
/// leaves the copies on `$PATH` unjudged, in the report as well as the exit
/// code: a run that has not tried to change something cannot have failed at it.
fn settled(reports: &[InstallReport]) -> bool {
    !reports
        .iter()
        .any(|r| matches!(r.action, Action::Failed | Action::Pending | Action::Planned))
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
        let current = install(Manager::Uv, "0.2.48", Source::Registry);
        assert_eq!(
            plan(&current, "0.2.48", Path::new(".")),
            Planned::AlreadyCurrent
        );
        // The negative control: one patch behind, and a command appears.
        let stale = install(Manager::Uv, "0.2.47", Source::Registry);
        assert!(matches!(
            plan(&stale, "0.2.48", Path::new(".")),
            Planned::Install(_)
        ));
    }

    #[test]
    fn a_path_install_of_this_project_is_rebuilt_from_it() {
        let here = std::env::current_dir().unwrap();
        let install = install(Manager::Cargo, "0.2.29", Source::Path(here.clone()));
        let Planned::Install(commands) = plan(&install, "0.2.48", &here) else {
            panic!("a path install of this project must be refreshed");
        };
        assert_eq!(
            commands[0][1..3],
            ["install".to_string(), "--path".to_string()]
        );
    }

    #[test]
    fn a_path_install_of_another_project_is_left_alone() {
        let install = install(
            Manager::Cargo,
            "0.2.29",
            Source::Path(PathBuf::from("/somewhere/else")),
        );
        let planned = plan(&install, "0.2.48", Path::new("/here"));
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
            plan(&install, "0.2.48", Path::new(".")),
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
        let package = Package::Named("rumdl".to_string());
        let commands = vec![vec!["cargo".to_string(), "install".to_string()]];
        let output = OutputConfig::new(crate::cli::OutputFormat::Json, false);

        let (report, after) = carry_out(
            &install,
            &package,
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
            &install,
            &package,
            "0.2.48",
            Decision::Run(commands.clone()),
            Some(Hold::DryRun),
            &output,
        );
        assert_eq!(report.action, Action::Planned);
        assert_eq!(report.detail, None);
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
            binary_names(&[uv, cargo]),
            vec!["rumdl".to_string(), "rumdl-lsp".to_string()]
        );
    }
}
