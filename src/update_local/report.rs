use serde_json::json;

use super::pathscan::{BinaryReport, Copy};
use super::{Action, InstallReport};
use crate::output::OutputConfig;

/// Width of the name column, matching `verify`'s report.
const NAME: usize = 10;

pub fn render(
    version: &str,
    ok: bool,
    dry_run: bool,
    installs: &[InstallReport],
    binaries: &[BinaryReport],
    settled: bool,
    output: &OutputConfig,
) {
    if output.is_json() {
        render_json(version, ok, dry_run, installs, binaries);
    } else {
        render_text(version, dry_run, installs, binaries, settled);
    }
}

fn render_json(
    version: &str,
    ok: bool,
    dry_run: bool,
    installs: &[InstallReport],
    binaries: &[BinaryReport],
) {
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
    println!(
        "{}",
        json!({
            "version": version,
            "ok": ok,
            "changed": changed,
            "dry_run": dry_run,
            "installs": installs,
            "binaries": binaries,
        })
    );
}

fn render_text(
    version: &str,
    dry_run: bool,
    installs: &[InstallReport],
    binaries: &[BinaryReport],
    settled: bool,
) {
    let suffix = if dry_run { " (dry run)" } else { "" };
    println!("update-local {version}{suffix}");
    if installs.is_empty() {
        println!("  ok   {:<NAME$} not installed locally", "-");
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

/// How a copy on `$PATH` is owned. An unmanaged copy carries no version,
/// because nothing on this machine can tell us one without running it.
fn describe(copy: &Copy) -> String {
    match (copy.manager, &copy.version) {
        (Some(m), Some(v)) => format!("{} {v}", m.name()),
        (Some(m), None) => m.name().to_string(),
        (None, _) => "unmanaged".to_string(),
    }
}
