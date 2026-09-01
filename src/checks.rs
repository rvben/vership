use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};
use crate::git;
use crate::output;
use crate::project::ProjectType;

pub struct CheckOptions {
    pub expected_branch: String,
    pub run_lint: bool,
    pub run_tests: bool,
    pub lint_command: Option<String>,
    pub test_command: Option<String>,
    /// When true, skip the "no uncommitted changes" check. Used when resuming
    /// an interrupted release where version files were already bumped.
    pub allow_uncommitted: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            expected_branch: "main".to_string(),
            run_lint: true,
            run_tests: true,
            lint_command: None,
            test_command: None,
            allow_uncommitted: false,
        }
    }
}

/// Run all pre-flight checks. Returns Ok(()) if all pass.
pub fn run_preflight(
    root: &Path,
    tag: &str,
    project: &dyn ProjectType,
    options: &CheckOptions,
) -> Result<()> {
    run_preflight_with_policy(root, tag, project, options, true, None, false)
}

/// Run preflight with an explicit policy for untracked, non-ignored files.
/// The separate argument preserves the stable `CheckOptions` public shape.
pub fn run_preflight_with_untracked_policy(
    root: &Path,
    tag: &str,
    project: &dyn ProjectType,
    options: &CheckOptions,
    allow_untracked: bool,
) -> Result<()> {
    run_preflight_with_policy(root, tag, project, options, allow_untracked, None, true)
}

pub(crate) fn run_preflight_with_policy(
    root: &Path,
    tag: &str,
    project: &dyn ProjectType,
    options: &CheckOptions,
    allow_untracked: bool,
    allowed_local_tag: Option<&str>,
    check_remote_tag: bool,
) -> Result<()> {
    // No uncommitted changes (skipped when resuming an interrupted release)
    if options.allow_uncommitted {
        output::print_check_pass("Uncommitted changes allowed (resuming interrupted release)");
    } else if git::has_tracked_changes(root)? {
        output::print_check_fail("Uncommitted changes detected");
        return Err(Error::CheckFailed(
            "commit or stash your changes before releasing".to_string(),
        ));
    }

    // A resume permits the tracked release files that an interrupted run left
    // behind, but unrelated untracked files remain subject to the same policy.
    let untracked = git::untracked_files(root)?;
    if !untracked.is_empty() && !allow_untracked {
        output::print_check_fail("Untracked files detected");
        let preview = untracked
            .iter()
            .take(5)
            .map(|path| format!("{path:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if untracked.len() > 5 {
            format!(" (and {} more)", untracked.len() - 5)
        } else {
            String::new()
        };
        return Err(Error::CheckFailed(format!(
            "add, ignore, or remove untracked files before releasing: {preview}{suffix}; set checks.allow_untracked = true to permit them"
        )));
    }
    if options.allow_uncommitted {
        if !untracked.is_empty() {
            output::print_check_pass(&format!(
                "{} untracked path(s) explicitly allowed",
                untracked.len()
            ));
        }
    } else if untracked.is_empty() {
        output::print_check_pass("Working tree is clean");
    } else {
        output::print_check_pass(&format!(
            "No tracked changes ({} untracked path(s) explicitly allowed)",
            untracked.len()
        ));
    }

    // On expected branch
    let branch = git::current_branch(root)?;
    if branch != options.expected_branch {
        output::print_check_fail(&format!(
            "On branch '{branch}', expected '{}'",
            options.expected_branch
        ));
        return Err(Error::CheckFailed(format!(
            "switch to '{}' branch before releasing",
            options.expected_branch
        )));
    }
    output::print_check_pass(&format!("On branch {branch}"));

    // Tag does not already exist. A duplicate tag is an incompatible-repeat
    // conflict: re-running cannot converge, so this gets its own error kind.
    if git::tag_exists(root, tag)? && allowed_local_tag != Some(tag) {
        output::print_check_fail(&format!("Tag {tag} already exists"));
        return Err(Error::Conflict(format!("tag {tag} already exists")));
    }
    if allowed_local_tag == Some(tag) {
        output::print_check_pass(&format!(
            "Tag {tag} exists locally and is confirmed unpublished"
        ));
    } else {
        output::print_check_pass(&format!("Tag {tag} does not exist"));
    }
    if check_remote_tag && git::remote_exists(root, "origin")? {
        if git::remote_tag_exists(root, tag)? {
            output::print_check_fail(&format!("Tag {tag} already exists on origin"));
            return Err(Error::Conflict(format!(
                "tag {tag} already exists on origin; fetch and inspect it before releasing"
            )));
        }
        output::print_check_pass(&format!("Tag {tag} does not exist on origin"));
    }

    // Lock file in sync
    match project.verify_lockfile(root) {
        Ok(()) => output::print_check_pass("Lock file in sync"),
        Err(e) => {
            output::print_check_fail("Lock file out of sync");
            return Err(e);
        }
    }

    // Lint (skippable)
    if options.run_lint {
        let result = if let Some(cmd) = &options.lint_command {
            run_shell_command(root, cmd)
        } else {
            project.run_lint(root)
        };
        match result {
            Ok(()) => output::print_check_pass("Lint passes"),
            Err(e) => {
                output::print_check_fail("Lint failed");
                return Err(e);
            }
        }
    }

    // Tests (skippable)
    if options.run_tests {
        let result = if let Some(cmd) = &options.test_command {
            run_shell_command(root, cmd)
        } else {
            project.run_tests(root)
        };
        match result {
            Ok(()) => output::print_check_pass("Tests pass"),
            Err(e) => {
                output::print_check_fail("Tests failed");
                return Err(e);
            }
        }
    }

    Ok(())
}

fn run_shell_command(root: &Path, cmd: &str) -> Result<()> {
    // Inherit stdout/stderr so a failed check leaves the actionable compiler,
    // test, or linter diagnostic visible. This streams output without buffering
    // an arbitrarily large test log in memory.
    let mut command = Command::new("sh");
    command.args(["-c", cmd]).current_dir(root);
    let status = crate::process::status_with_stdout_to_stderr(&mut command)
        .map_err(|e| Error::Other(format!("run command '{cmd}': {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CheckFailed(format!("command failed: {cmd}")))
    }
}
