use std::path::{Path, PathBuf};

use semver::Version;

use crate::artifacts;
use crate::changelog;
use crate::checks::{self, CheckOptions};
use crate::cli::BumpLevel;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::git;
use crate::hooks;
use crate::output::{self, OutputConfig};
use crate::project;
use crate::version;
use crate::version_files;

/// Pure version logic for resume detection.
///
/// Returns `Some((committed_version, target_version))` when `on_disk` already
/// equals what bumping `latest_tag` by `level` would produce, indicating a
/// previous run wrote version files but did not commit. Returns `None` otherwise.
fn resume_versions(
    on_disk: &Version,
    latest_tag: Option<&str>,
    level: BumpLevel,
) -> Option<(Version, Version)> {
    let tag_str = latest_tag?;
    let tag_version = Version::parse(tag_str.trim_start_matches('v')).ok()?;
    let expected_new = version::bump(tag_version.clone(), level);
    (on_disk == &expected_new).then_some((tag_version, expected_new))
}

/// Detect whether a previous `bump` run was interrupted after writing version
/// files but before committing. Returns `(current_version, new_version, resuming)`.
///
/// Detection logic: if the on-disk version already equals what a `level` bump
/// from the last git tag would produce, AND the working tree has uncommitted
/// changes, the previous run was interrupted mid-flight.
fn detect_resume(
    on_disk: &Version,
    latest_tag: Option<&str>,
    level: BumpLevel,
    root: &Path,
) -> Result<(Version, Version, bool)> {
    if let Some((current, new)) = resume_versions(on_disk, latest_tag, level)
        && git::has_uncommitted_changes(root)?
    {
        return Ok((current, new, true));
    }
    let new = version::bump(on_disk.clone(), level);
    Ok((on_disk.clone(), new, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resume_versions_detects_interrupted_patch_bump() {
        // Simulates: last tag v0.1.70, files already bumped to 0.1.71
        let on_disk = Version::parse("0.1.71").unwrap();
        let result = resume_versions(&on_disk, Some("v0.1.70"), BumpLevel::Patch);
        let (current, new) = result.expect("should detect resume");
        assert_eq!(current, Version::parse("0.1.70").unwrap());
        assert_eq!(new, Version::parse("0.1.71").unwrap());
    }

    #[test]
    fn test_resume_versions_no_resume_when_not_bumped() {
        // Normal case: last tag v0.1.70, files still say 0.1.70
        let on_disk = Version::parse("0.1.70").unwrap();
        assert!(resume_versions(&on_disk, Some("v0.1.70"), BumpLevel::Patch).is_none());
    }

    #[test]
    fn test_resume_versions_no_resume_when_no_tag() {
        // First release: no prior tag
        let on_disk = Version::parse("0.1.0").unwrap();
        assert!(resume_versions(&on_disk, None, BumpLevel::Patch).is_none());
    }

    #[test]
    fn test_resume_versions_no_resume_when_wrong_level() {
        // Files bumped to minor (0.2.0) but caller asks for patch bump
        let on_disk = Version::parse("0.2.0").unwrap();
        assert!(
            resume_versions(&on_disk, Some("v0.1.70"), BumpLevel::Patch).is_none(),
            "patch bump from v0.1.70 gives 0.1.71, not 0.2.0"
        );
    }

    #[test]
    fn test_resume_versions_detects_minor_bump() {
        let on_disk = Version::parse("0.2.0").unwrap();
        let result = resume_versions(&on_disk, Some("v0.1.70"), BumpLevel::Minor);
        assert!(result.is_some(), "minor bump from 0.1.70 gives 0.2.0");
    }

    #[test]
    fn test_resume_versions_detects_major_bump() {
        let on_disk = Version::parse("1.0.0").unwrap();
        let result = resume_versions(&on_disk, Some("v0.9.5"), BumpLevel::Major);
        let (current, new) = result.expect("should detect resume");
        assert_eq!(current, Version::parse("0.9.5").unwrap());
        assert_eq!(new, Version::parse("1.0.0").unwrap());
    }
}

fn project_root() -> Result<PathBuf> {
    std::env::current_dir()
        .map_err(|e| Error::Other(format!("failed to get current directory: {e}")))
}

pub fn status(output: &OutputConfig) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    let commits = git::commits_since_tag(&root, latest_tag.as_deref())?;

    if output.json {
        let data = serde_json::json!({
            "project_type": project.name(),
            "current_version": current_version.to_string(),
            "latest_tag": latest_tag,
            "unreleased_commits": commits.len(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&data).expect("serialize")
        );
    } else {
        eprintln!("Project type: {}", project.name());
        eprintln!("Current version: {current_version}");
        if let Some(tag) = &latest_tag {
            eprintln!("Latest tag: {tag}");
        } else {
            eprintln!("Latest tag: (none)");
        }
        eprintln!("Unreleased commits: {}", commits.len());

        if !commits.is_empty() {
            eprintln!();
            for c in &commits {
                let short_hash = &c.hash[..7.min(c.hash.len())];
                eprintln!("  {short_hash} {}", c.message);
            }
        }
    }

    Ok(())
}

pub fn preflight() -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let new_version = version::bump(current_version, BumpLevel::Patch);
    let tag = format!("v{new_version}");

    let options = CheckOptions {
        expected_branch: config.project.branch.clone(),
        run_lint: config.checks.lint,
        run_tests: config.checks.tests,
        lint_command: config.checks.lint_command.clone(),
        test_command: config.checks.test_command.clone(),
        allow_uncommitted: false,
    };

    checks::run_preflight(&root, &tag, project.as_ref(), &options)?;
    eprintln!("\nAll checks passed. Ready to release.");
    Ok(())
}

pub fn changelog_preview() -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    let commits = git::commits_since_tag(&root, latest_tag.as_deref())?;
    let remote_url = git::remote_url(&root)?;

    let next_version = version::bump(current_version, BumpLevel::Patch);
    let changelog_section = changelog::generate_changelog(
        &commits,
        &next_version.to_string(),
        latest_tag.as_deref(),
        remote_url.as_deref(),
    );

    println!("{changelog_section}");
    Ok(())
}

pub fn bump(
    level: BumpLevel,
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    force_resume: bool,
) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;

    // Hoist latest_tag early so resume detection and changelog generation share it.
    let latest_tag = git::latest_semver_tag(&root)?;

    // Calculate versions, detecting whether a previous run was interrupted.
    let on_disk_version = project.read_version(&root)?;
    let (current_version, new_version, resuming) = if force_resume {
        // Explicit resume: treat the on-disk version as the target and derive
        // the previous version from the last git tag (falling back to on-disk).
        let current = latest_tag
            .as_deref()
            .and_then(|t| Version::parse(t.trim_start_matches('v')).ok())
            .unwrap_or_else(|| on_disk_version.clone());
        (current, on_disk_version.clone(), true)
    } else {
        detect_resume(&on_disk_version, latest_tag.as_deref(), level, &root)?
    };
    let tag = format!("v{new_version}");

    if resuming {
        output::print_step(&format!(
            "Resuming interrupted release: {current_version} → {new_version}"
        ));
    }

    // Pre-flight checks
    let options = CheckOptions {
        expected_branch: config.project.branch.clone(),
        run_lint: if skip_checks {
            false
        } else {
            config.checks.lint
        },
        run_tests: if skip_checks {
            false
        } else {
            config.checks.tests
        },
        lint_command: config.checks.lint_command.clone(),
        test_command: config.checks.test_command.clone(),
        allow_uncommitted: resuming,
    };
    checks::run_preflight(&root, &tag, project.as_ref(), &options)?;

    // Pre-bump hook
    if !dry_run {
        hooks::run_hook(&root, "pre-bump", config.hooks.pre_bump.as_deref())?;
    }

    // Bump version in project files (skip when resuming — already done)
    if resuming {
        output::print_step(&format!(
            "Bumping {current_version} → {new_version} (already applied, skipping)"
        ));
    } else {
        output::print_step(&format!("Bumping {current_version} → {new_version}"));
        if !dry_run {
            project.write_version(&root, &new_version)?;
            project.sync_lockfile(&root)?;
        }
        output::print_step(&format!("Updated {}", project.name().to_lowercase()));
    }

    // Update version references in extra files (idempotent: no-op if already applied)
    let vf_touched = if !dry_run && !config.version_files.is_empty() {
        output::print_step("Updating version files");
        version_files::apply(
            &root,
            &config.version_files,
            &current_version.to_string(),
            &new_version.to_string(),
        )?
    } else {
        Vec::new()
    };

    // Generate changelog
    let commits = git::commits_since_tag(&root, latest_tag.as_deref())?;
    let remote_url = git::remote_url(&root)?;

    let changelog_section = changelog::generate_changelog_with_mode(
        &commits,
        &new_version.to_string(),
        latest_tag.as_deref(),
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;

    let changelog_path = root.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&changelog_path).ok();

    // Guard against duplicate sections when resuming after changelog was written
    let changelog_already_written = existing
        .as_deref()
        .is_some_and(|c| changelog::version_exists_in_changelog(c, &new_version.to_string()));
    let full_changelog = if changelog_already_written {
        existing.clone().unwrap_or_default()
    } else {
        changelog::prepend_to_changelog(existing.as_deref(), &changelog_section)
    };

    let entry_count = commits
        .iter()
        .filter_map(|c| changelog::parse_conventional_commit(&c.message))
        .filter(|cc| matches!(cc.commit_type.as_str(), "feat" | "fix" | "perf" | "change"))
        .count();
    if changelog_already_written {
        output::print_step(&format!(
            "Changelog already up-to-date ({entry_count} entries)"
        ));
    } else {
        output::print_step(&format!("Generated changelog ({entry_count} entries)"));
    }

    if dry_run {
        eprintln!("\n--- Dry run: no changes made ---");
        eprintln!("\nChangelog preview:\n");
        eprintln!("{changelog_section}");
        return Ok(());
    }

    std::fs::write(&changelog_path, &full_changelog)?;

    // Run artifact generation commands
    let artifact_files = if !config.artifacts.is_empty() {
        artifacts::run(&root, &config.artifacts)?
    } else {
        Vec::new()
    };

    // Post-bump hook
    hooks::run_hook(&root, "post-bump", config.hooks.post_bump.as_deref())?;

    // Stage modified files
    let modified: Vec<String> = project
        .modified_files()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let vf_strings: Vec<String> = vf_touched
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let af_strings: Vec<String> = artifact_files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let mut stage_refs: Vec<&str> = modified.iter().map(|s| s.as_str()).collect();
    stage_refs.push("CHANGELOG.md");
    stage_refs.extend(vf_strings.iter().map(|s| s.as_str()));
    stage_refs.extend(af_strings.iter().map(|s| s.as_str()));
    git::stage_files(&root, &stage_refs)?;

    // Commit
    let commit_msg = if project.is_tag_versioned() {
        format!("chore: release {tag}")
    } else {
        format!("chore: bump version to {tag}")
    };
    git::commit(&root, &commit_msg)?;
    output::print_step(&format!("Committed: {commit_msg}"));

    // Tag
    git::create_tag(&root, &tag)?;
    output::print_step(&format!("Tagged: {tag}"));

    if no_push {
        output::print_step(&format!("Ready to push: git push origin main {tag}"));
        return Ok(());
    }

    // Pre-push hook
    hooks::run_hook(&root, "pre-push", config.hooks.pre_push.as_deref())?;

    // Push branch and tag
    let branch = git::current_branch(&root)?;
    git::push_with_tag(&root, &branch, &tag)?;
    output::print_step("Pushed to origin");

    // Post-push hook
    hooks::run_hook(&root, "post-push", config.hooks.post_push.as_deref())?;

    Ok(())
}
