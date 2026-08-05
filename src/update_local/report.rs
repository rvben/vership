use serde_json::json;

use super::managers::Manager;
use super::pathscan::{BinaryReport, Copy};
use super::{Action, InstallReport};
use crate::output::OutputConfig;

/// Width of the name column, matching `verify`'s report.
const NAME: usize = 10;

/// Everything one `update-local` pass has to say about itself.
pub struct Report<'a> {
    pub version: &'a str,
    pub ok: bool,
    pub dry_run: bool,
    pub installs: &'a [InstallReport],
    pub binaries: &'a [BinaryReport],
    /// Whether every install has reached its final state. An outstanding one
    /// leaves the copies on `$PATH` reported without a verdict.
    pub settled: bool,
    /// What each manager was asked about, whether or not it held anything.
    pub considered: &'a [(Manager, Vec<String>)],
}

pub fn render(report: &Report<'_>, output: &OutputConfig) {
    if output.is_json() {
        render_json(report);
    } else {
        render_text(report);
    }
}

fn render_json(report: &Report<'_>) {
    let Report {
        version,
        ok,
        dry_run,
        installs,
        binaries,
        considered,
        ..
    } = *report;
    let changed = installs.iter().any(|i| i.action == Action::Updated);
    let installs: Vec<_> = installs
        .iter()
        .map(|i| {
            json!({
                "manager": i.manager.name(),
                "package": i.package,
                "before": i.before,
                "after": i.after,
                "action": i.action.name(),
                "detail": i.detail,
                "commands": i.commands,
            })
        })
        .collect();
    let binaries: Vec<_> = binaries
        .iter()
        .map(|b| {
            let winner = b.winner();
            json!({
                "name": b.name,
                "path": winner.map(|c| c.path.display().to_string()),
                "manager": winner.and_then(|c| c.manager).map(|m| m.name()),
                "version": winner.and_then(|c| c.version.clone()),
                "shadowed": b.shadowed().iter().map(|c| json!({
                    "path": c.path.display().to_string(),
                    "manager": c.manager.map(|m| m.name()),
                    "version": c.version,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    // What each manager was asked about. An empty `installs` otherwise reads
    // the same whether this project has nothing a manager could hold or has
    // something that is simply not installed.
    let considered: Vec<_> = considered
        .iter()
        .map(|(manager, names)| {
            json!({
                "manager": manager.name(),
                "packages": names,
            })
        })
        .collect();
    println!(
        "{}",
        json!({
            "version": version,
            "ok": ok,
            "changed": changed,
            "dry_run": dry_run,
            "installs": installs,
            "binaries": binaries,
            "considered": considered,
        })
    );
}

fn render_text(report: &Report<'_>) {
    let Report {
        version,
        dry_run,
        installs,
        binaries,
        settled,
        considered,
        ..
    } = *report;
    let suffix = if dry_run { " (dry run)" } else { "" };
    println!("update-local {version}{suffix}");
    if installs.is_empty() {
        // Naming what was asked about keeps this line from claiming more than
        // it knows. "Not installed locally" is a statement about packages that
        // were looked for, and with nothing to look for it would be asserting
        // an absence that was never checked.
        let detail = match considered.is_empty() {
            true => "no cargo, uv, npm or brew package found in this project".to_string(),
            false => format!(
                "not installed locally (looked for {})",
                describe_considered(considered)
            ),
        };
        println!("  ok   {:<NAME$} {detail}", "-");
    }
    for i in installs {
        println!(
            "  {} {:<NAME$} {}",
            mark(i.action),
            i.manager.name(),
            install_detail(i)
        );
        for argv in &i.commands {
            println!("  {:NAME$}      {}", "", argv.join(" "));
        }
    }
    for b in binaries {
        let (mark, detail) = match b.winner() {
            None => ("warn", "not on PATH".to_string()),
            Some(c) => {
                // With an install still outstanding this line is the starting
                // state, not the result, so it is reported without a verdict.
                let mark = match (settled, c.version.as_deref() == Some(version)) {
                    (false, _) => "wait",
                    (true, true) => "ok  ",
                    (true, false) => "FAIL",
                };
                (mark, format!("{} ({})", c.path.display(), describe(c)))
            }
        };
        println!("  {mark} {:<NAME$} {detail}", b.name);
        for c in b.shadowed() {
            println!(
                "  {:NAME$}      shadowed {} ({})",
                "",
                c.path.display(),
                describe(c)
            );
        }
    }
}

fn mark(action: Action) -> &'static str {
    match action {
        Action::Updated | Action::AlreadyCurrent => "ok  ",
        Action::Planned => "plan",
        Action::Pending => "wait",
        Action::Skipped => "skip",
        Action::Failed => "FAIL",
    }
}

fn install_detail(i: &InstallReport) -> String {
    let versions = match &i.after {
        Some(after) if after != &i.before => format!("{} -> {after}", i.before),
        _ => i.before.clone(),
    };
    match &i.detail {
        Some(detail) => format!("{versions}  {detail}"),
        None => versions,
    }
}

/// The packages each manager was asked about, as `cargo husker, husker-core`.
/// A workspace contributes one name per member, so this is capped: past a few
/// names the list stops being something a reader takes in, and the JSON report
/// carries the full set for anything that needs it.
fn describe_considered(considered: &[(Manager, Vec<String>)]) -> String {
    const SHOWN: usize = 3;
    considered
        .iter()
        .map(|(manager, names)| {
            let shown = names
                .iter()
                .take(SHOWN)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            match names.len().saturating_sub(SHOWN) {
                0 => format!("{} {shown}", manager.name()),
                more => format!("{} {shown} +{more} more", manager.name()),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// How a copy on `$PATH` is owned. An unmanaged copy carries no version,
/// because nothing on this machine can tell us one without running it.
fn describe(copy: &Copy) -> String {
    match (copy.manager, &copy.version) {
        (Some(m), Some(v)) => format!("{} {v}", m.name()),
        (Some(m), None) => m.name().to_string(),
        (None, _) => "unmanaged".to_string(),
    }
}
