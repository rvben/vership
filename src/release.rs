use std::path::PathBuf;

use crate::artifacts;
use crate::changelog;
use crate::checks::{self, CheckOptions};
use crate::cli::BumpLevel;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::git;
use crate::hooks;
use crate::output::{self, OutputConfig};
use crate::planning::{Mutation, ReleasePlan};
use crate::project;
use crate::version;
use crate::version_files;

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

/// Options that control how a `ReleasePlan` is executed.
pub struct ExecOpts {
    pub dry_run: bool,
    pub skip_checks: bool,
    pub no_push: bool,
}

/// Bump the version per `level` and release.
///
/// Auto-detects an interrupted prior run: if the manifest is already at the
/// expected post-bump version and the working tree is dirty, finishes that
/// run instead of double-bumping.
pub fn bump(level: BumpLevel, dry_run: bool, skip_checks: bool, no_push: bool) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;

    let on_disk = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    let has_uncommitted = git::has_uncommitted_changes(&root)?;

    let plan = ReleasePlan::bump(on_disk, latest_tag.as_deref(), level, has_uncommitted);
    execute(
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
    )
}

/// Tag the on-disk version as-is.
///
/// Used for initial releases (when the manifest is already at the intended
/// starting version) or when the version was set manually.
pub fn release_current(dry_run: bool, skip_checks: bool, no_push: bool) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;

    let on_disk = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;

    let plan = ReleasePlan::release_current(on_disk, latest_tag.as_deref())?;
    execute(
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
    )
}

/// Resume an interrupted bump.
///
/// Trusts the on-disk version as the intended target, then completes the
/// commit/tag/push flow.
pub fn resume(dry_run: bool, skip_checks: bool, no_push: bool) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;

    let on_disk = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;

    let plan = ReleasePlan::resume(on_disk, latest_tag.as_deref())?;
    execute(
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
    )
}

/// Single linear orchestrator. Runs preflight, optionally writes the version,
/// generates changelog, commits, tags, and pushes.
fn execute(plan: ReleasePlan, opts: ExecOpts) -> Result<()> {
    let root = project_root()?;
    let config = Config::load(&root.join("vership.toml"));
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let tag = plan.tag();

    if plan.allow_dirty_tree {
        output::print_step(&format!(
            "Resuming interrupted release: target {}",
            plan.target
        ));
    }

    // Pre-flight
    let check_options = CheckOptions {
        expected_branch: config.project.branch.clone(),
        run_lint: !opts.skip_checks && config.checks.lint,
        run_tests: !opts.skip_checks && config.checks.tests,
        lint_command: config.checks.lint_command.clone(),
        test_command: config.checks.test_command.clone(),
        allow_uncommitted: plan.allow_dirty_tree,
    };
    checks::run_preflight(&root, &tag, project.as_ref(), &check_options)?;

    // Pre-bump hook
    if !opts.dry_run {
        hooks::run_hook(&root, "pre-bump", config.hooks.pre_bump.as_deref())?;
    }

    // Mutation: write version into manifest + apply version_files (when planned).
    let on_disk = project.read_version(&root)?;
    let vf_touched = match plan.mutation {
        Mutation::Bump => {
            output::print_step(&format!("Bumping {on_disk} → {}", plan.target));
            if !opts.dry_run {
                project.write_version(&root, &plan.target)?;
                project.sync_lockfile(&root)?;
            }
            output::print_step(&format!("Updated {}", project.name().to_lowercase()));

            if !opts.dry_run && !config.version_files.is_empty() {
                output::print_step("Updating version files");
                version_files::apply(
                    &root,
                    &config.version_files,
                    &on_disk.to_string(),
                    &plan.target.to_string(),
                )?
            } else {
                Vec::new()
            }
        }
        Mutation::None if plan.allow_dirty_tree => {
            output::print_step(&format!(
                "Bumping {on_disk} → {} (already applied, skipping)",
                plan.target
            ));
            Vec::new()
        }
        Mutation::None => {
            output::print_step(&format!(
                "Releasing current version {} (no manifest change)",
                plan.target
            ));
            Vec::new()
        }
    };

    // Changelog
    let commits = git::commits_since_tag(&root, plan.previous_tag.as_deref())?;
    let remote_url = git::remote_url(&root)?;

    let changelog_section = changelog::generate_changelog_with_mode(
        &commits,
        &plan.target.to_string(),
        plan.previous_tag.as_deref(),
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;

    let changelog_path = root.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&changelog_path).ok();

    // Guard against duplicate sections when resuming after changelog was written.
    let changelog_already_written = existing
        .as_deref()
        .is_some_and(|c| changelog::version_exists_in_changelog(c, &plan.target.to_string()));
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

    if opts.dry_run {
        eprintln!("\n--- Dry run: no changes made ---");
        eprintln!("\nChangelog preview:\n");
        eprintln!("{changelog_section}");
        return Ok(());
    }

    std::fs::write(&changelog_path, &full_changelog)?;

    let artifact_files = if !config.artifacts.is_empty() {
        artifacts::run(&root, &config.artifacts)?
    } else {
        Vec::new()
    };

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

    let commit_msg = if project.is_tag_versioned() {
        format!("chore: release {tag}")
    } else {
        format!("chore: bump version to {tag}")
    };
    git::commit(&root, &commit_msg)?;
    output::print_step(&format!("Committed: {commit_msg}"));

    git::create_tag(&root, &tag)?;
    output::print_step(&format!("Tagged: {tag}"));

    if opts.no_push {
        output::print_step(&format!("Ready to push: git push origin main {tag}"));
        return Ok(());
    }

    hooks::run_hook(&root, "pre-push", config.hooks.pre_push.as_deref())?;

    let branch = git::current_branch(&root)?;
    git::push_with_tag(&root, &branch, &tag)?;
    output::print_step("Pushed to origin");

    hooks::run_hook(&root, "post-push", config.hooks.post_push.as_deref())?;

    Ok(())
}
