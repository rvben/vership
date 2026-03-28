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
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            expected_branch: "main".to_string(),
            run_lint: true,
            run_tests: true,
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
    // No uncommitted changes
    if git::has_uncommitted_changes(root)? {
        output::print_check_fail("Uncommitted changes detected");
        return Err(Error::CheckFailed(
            "commit or stash your changes before releasing".to_string(),
        ));
    }
    output::print_check_pass("No uncommitted changes");

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

    // Tag does not already exist
    if git::tag_exists(root, tag)? {
        output::print_check_fail(&format!("Tag {tag} already exists"));
        return Err(Error::CheckFailed(format!("tag {tag} already exists")));
    }
    output::print_check_pass(&format!("Tag {tag} does not exist"));

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
        let fmt_ok = Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo fmt: {e}")))?
            .success();

        let clippy_ok = Command::new("cargo")
            .args(["clippy", "--", "-D", "warnings"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo clippy: {e}")))?
            .success();

        if !fmt_ok || !clippy_ok {
            output::print_check_fail("Lint failed");
            return Err(Error::CheckFailed("lint checks failed".to_string()));
        }
        output::print_check_pass("Lint passes");
    }

    // Tests (skippable)
    if options.run_tests {
        let test_ok = Command::new("cargo")
            .args(["test", "--quiet"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Other(format!("run cargo test: {e}")))?
            .success();

        if !test_ok {
            output::print_check_fail("Tests failed");
            return Err(Error::CheckFailed("tests failed".to_string()));
        }
        output::print_check_pass("Tests pass");
    }

    Ok(())
}
