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

pub fn status(
    output: &OutputConfig,
    limit: usize,
    offset: usize,
    fields: Option<&str>,
) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let package_name = project.package_name(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    let all_commits = git::commits_since_tag(&root, latest_tag.as_deref())?;

    // Apply offset then limit to the commit list.
    let after_offset = if offset < all_commits.len() {
        &all_commits[offset..]
    } else {
        &[]
    };
    let shown_commits = if limit > 0 {
        &after_offset[..limit.min(after_offset.len())]
    } else {
        after_offset
    };

    if output.is_json() {
        let mut data = serde_json::json!({
            "project_type": project.name(),
            "current_version": current_version.to_string(),
            "latest_tag": latest_tag,
            "unreleased_commits": all_commits.len(),
        });
        if let Some(name) = &package_name {
            data["name"] = serde_json::json!(name);
        }

        // Build the complete document first, then apply --fields. Filtering
        // before the computed fields exist would make schema-declared fields
        // like `commits` unselectable.
        let commits_json: Vec<serde_json::Value> = shown_commits
            .iter()
            .map(|c| {
                serde_json::json!({
                    "hash": &c.hash[..7.min(c.hash.len())],
                    "message": c.subject(),
                })
            })
            .collect();
        let total = all_commits.len();
        let truncated = (limit > 0 && after_offset.len() > limit) || offset > 0;
        data["commits"] = serde_json::json!(commits_json);
        if truncated {
            data["truncated"] = serde_json::json!(true);
            data["total_commits"] = serde_json::json!(total);
        }
        if limit > 0 {
            data["limit"] = serde_json::json!(limit);
        }
        if offset > 0 {
            data["offset"] = serde_json::json!(offset);
        }

        if let Some(f) = fields {
            let keep: std::collections::HashSet<&str> = f.split(',').collect();
            if let Some(obj) = data.as_object_mut() {
                obj.retain(|k, _| keep.contains(k.as_str()));
            }
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&data).expect("serialize")
        );
    } else {
        // Text mode: data goes to stdout per the spec (Principle 3).
        println!("Project type: {}", project.name());
        if let Some(name) = &package_name {
            println!("Package: {name}");
        }
        println!("Current version: {current_version}");
        if let Some(tag) = &latest_tag {
            println!("Latest tag: {tag}");
        } else {
            println!("Latest tag: (none)");
        }
        println!("Unreleased commits: {}", all_commits.len());

        if !shown_commits.is_empty() {
            println!();
            for c in shown_commits {
                let short_hash = &c.hash[..7.min(c.hash.len())];
                println!("  {short_hash} {}", c.subject());
            }
            if limit > 0 && after_offset.len() > limit {
                println!("  ... ({} more)", after_offset.len() - limit);
            }
        }
    }

    Ok(())
}

/// Run preflight for the default patch target.
pub fn preflight() -> Result<()> {
    preflight_for(BumpLevel::Patch)
}

/// Run preflight for the requested release target.
pub fn preflight_for(level: BumpLevel) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let new_version = version::bump(current_version, level);
    let tag = format!("v{new_version}");

    let options = CheckOptions {
        expected_branch: config.project.branch.clone(),
        run_lint: config.checks.lint,
        run_tests: config.checks.tests,
        lint_command: config.checks.lint_command.clone(),
        test_command: config.checks.test_command.clone(),
        allow_untracked: config.checks.allow_untracked,
        allow_uncommitted: false,
    };

    checks::run_preflight(&root, &tag, project.as_ref(), &options)?;
    eprintln!("\nAll checks passed. Ready to release.");
    Ok(())
}

/// Preview the default patch release section.
pub fn changelog_preview() -> Result<()> {
    changelog_preview_for(BumpLevel::Patch)
}

/// Preview the exact release section for the requested level.
pub fn changelog_preview_for(level: BumpLevel) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
    let project = project::detect(&root, config.project.project_type.as_deref())?;
    let current_version = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    let commits = git::commits_since_tag(&root, latest_tag.as_deref())?;
    let remote_url = git::remote_url(&root)?;

    let next_version = version::bump(current_version, level);
    let generated = changelog::generate_changelog_with_mode(
        &commits,
        &next_version.to_string(),
        latest_tag.as_deref(),
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;

    let existing = std::fs::read_to_string(root.join("CHANGELOG.md")).ok();
    let update = changelog::integrate_changelog_checked(existing.as_deref(), &generated)
        .map_err(Error::CheckFailed)?;
    if update.promoted {
        output::print_step(&format!(
            "Previewing curated Unreleased notes ({} generated entries replaced)",
            update.replaced_generated_entries
        ));
    }
    let changelog_section = changelog::extract_section(&update.content, &next_version.to_string())
        .unwrap_or(&generated);

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
    bump_with_prepare(level, dry_run, skip_checks, no_push, false)
}

pub fn bump_with_prepare(
    level: BumpLevel,
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
    let project = project::detect(&root, config.project.project_type.as_deref())?;

    let on_disk = project.read_version(&root)?;
    let latest_tag = git::latest_semver_tag(&root)?;
    // Resume detection is based on tracked release files only. An unrelated
    // untracked path must never turn a fresh bump into an interrupted release.
    let has_uncommitted = git::has_tracked_changes(&root)?;

    let plan = ReleasePlan::bump(on_disk, latest_tag.as_deref(), level, has_uncommitted);
    execute(
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
        prepare,
    )
}

/// Tag the on-disk version as-is.
///
/// Used for initial releases (when the manifest is already at the intended
/// starting version) or when the version was set manually.
pub fn release_current(dry_run: bool, skip_checks: bool, no_push: bool) -> Result<()> {
    release_current_with_prepare(dry_run, skip_checks, no_push, false)
}

pub fn release_current_with_prepare(
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
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
        prepare,
    )
}

/// Resume an interrupted bump.
///
/// Trusts the on-disk version as the intended target, then completes the
/// commit/tag/push flow.
pub fn resume(dry_run: bool, skip_checks: bool, no_push: bool) -> Result<()> {
    resume_with_prepare(dry_run, skip_checks, no_push, false)
}

pub fn resume_with_prepare(
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
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
        prepare,
    )
}

/// Single linear orchestrator. Runs preflight, optionally writes the version,
/// generates changelog, commits, tags, and pushes.
fn execute(plan: ReleasePlan, opts: ExecOpts, prepare: bool) -> Result<()> {
    let root = project_root()?;
    let config = Config::load_checked(&root.join("vership.toml"))?;
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
        allow_untracked: config.checks.allow_untracked,
        allow_uncommitted: plan.allow_dirty_tree,
    };
    checks::run_preflight(&root, &tag, project.as_ref(), &check_options)?;

    let changelog_path = root.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&changelog_path).ok();
    let release_commit_already_prepared = plan.mutation == Mutation::None
        && !git::has_tracked_changes(&root)?
        && existing.as_deref().is_some_and(|content| {
            changelog::version_exists_in_changelog(content, &plan.target.to_string())
        });

    // A clean tree with the target version already present in the changelog is
    // the state produced by `--prepare` (and by an interrupted run after its
    // commit succeeded). Do not replay bump hooks or artifact generators: they
    // may have external or non-idempotent side effects. Only the tag/push phase
    // remains.
    if release_commit_already_prepared {
        output::print_step("Nothing to commit (release commit already exists)");
        if opts.dry_run {
            eprintln!("\n--- Dry run: no changes made ---");
            if let Some(preview) = existing
                .as_deref()
                .and_then(|content| changelog::extract_section(content, &plan.target.to_string()))
            {
                eprintln!("\nChangelog preview:\n");
                eprintln!("{preview}");
            }
            return Ok(());
        }
        return finish_release(&root, &config, &tag, opts.no_push, prepare);
    }

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
                "Bumping {on_disk} → {} (already applied)",
                plan.target
            ));
            // The interrupted run already wrote the version to disk, but the
            // manifest, lockfile, and configured version files may still be
            // uncommitted. Re-apply all of them (a no-op when the value already
            // matches, since on resume on-disk == target) so they are staged and
            // the release commit and tag carry the version change across every
            // file, not just the manifest. version_files::apply returns the
            // touched paths so they reach stage_files below.
            if !opts.dry_run {
                project.write_version(&root, &plan.target)?;
                project.sync_lockfile(&root)?;
                if !config.version_files.is_empty() {
                    output::print_step("Updating version files");
                    version_files::apply(
                        &root,
                        &config.version_files,
                        &plan.target.to_string(),
                        &plan.target.to_string(),
                    )?
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
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

    // Guard against duplicate sections when resuming after changelog was written.
    let changelog_already_written = existing
        .as_deref()
        .is_some_and(|c| changelog::version_exists_in_changelog(c, &plan.target.to_string()));
    let (full_changelog, promoted, replaced_generated_entries) = if changelog_already_written {
        (existing.clone().unwrap_or_default(), false, 0)
    } else {
        let update =
            changelog::integrate_changelog_checked(existing.as_deref(), &changelog_section)
                .map_err(Error::CheckFailed)?;
        (
            update.content,
            update.promoted,
            update.replaced_generated_entries,
        )
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
    } else if promoted {
        output::print_step(&format!(
            "Promoted curated Unreleased notes ({replaced_generated_entries} generated entries replaced)"
        ));
    } else {
        output::print_step(&format!("Generated changelog ({entry_count} entries)"));
    }

    if opts.dry_run {
        eprintln!("\n--- Dry run: no changes made ---");
        eprintln!("\nChangelog preview:\n");
        // Show the section as it will actually land, so a promoted `[Unreleased]`
        // block (curated content) previews correctly rather than the generated draft.
        let preview = changelog::extract_section(&full_changelog, &plan.target.to_string())
            .unwrap_or(&changelog_section);
        eprintln!("{preview}");
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

    // A resumed or retagged release may have nothing left to commit: the
    // version bump and changelog already landed in a prior run. Skip the
    // empty commit and proceed to the tag, which is the step that remains.
    if git::has_staged_changes(&root)? {
        let commit_msg = if project.is_tag_versioned() {
            format!("chore: release {tag}")
        } else {
            format!("chore: bump version to {tag}")
        };
        git::commit(&root, &commit_msg)?;
        output::print_step(&format!("Committed: {commit_msg}"));
    } else {
        output::print_step("Nothing to commit (release commit already exists)");
    }

    finish_release(&root, &config, &tag, opts.no_push, prepare)
}

fn finish_release(
    root: &std::path::Path,
    config: &Config,
    tag: &str,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    if prepare {
        output::print_step(&format!(
            "Prepared release commit for {tag}; review it, then run `vership release` to tag and push"
        ));
        return Ok(());
    }

    git::create_tag(root, tag)?;
    output::print_step(&format!("Tagged: {tag}"));

    if no_push {
        let branch = git::current_branch(root)?;
        output::print_step(&format!("Ready to push: git push origin {branch} {tag}"));
        return Ok(());
    }

    hooks::run_hook(root, "pre-push", config.hooks.pre_push.as_deref())?;

    let branch = git::current_branch(root)?;
    git::push_with_tag(root, &branch, tag)?;
    output::print_step("Pushed to origin");

    hooks::run_hook(root, "post-push", config.hooks.post_push.as_deref())?;

    Ok(())
}
