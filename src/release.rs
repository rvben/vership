use crate::context::ReleaseContext;
use std::path::Path;

use crate::artifacts;
use crate::changelog::{self, ChangelogIntegration, CuratedPolicy};
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

fn read_optional_text(path: &std::path::Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

fn unpublished_release_tag(
    ctx: &ReleaseContext,
    version: &semver::Version,
) -> Result<Option<String>> {
    let root = &ctx.repo;
    let tag = ctx.tag(version);
    if !git::tag_exists(root, &tag)? {
        return Ok(None);
    }
    let release_marker = format!("Vership-Release: {tag}");
    let marked_release = git::tag_message(root, &tag)?
        .is_some_and(|message| message.lines().any(|line| line.trim() == release_marker));
    if marked_release && git::tag_points_to_head(root, &tag)? {
        if git::remote_exists(root, "origin")? && git::remote_tag_exists(root, &tag)? {
            return Ok(None);
        }
        return Ok(Some(tag));
    }
    Ok(None)
}

pub fn status(
    output: &OutputConfig,
    limit: usize,
    offset: usize,
    fields: Option<&str>,
) -> Result<()> {
    status_in(
        &ReleaseContext::resolve(None)?,
        output,
        limit,
        offset,
        fields,
    )
}

pub(crate) fn status_in(
    ctx: &ReleaseContext,
    output: &OutputConfig,
    limit: usize,
    offset: usize,
    fields: Option<&str>,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let project = project::detect(root, config.project.project_type.as_deref())?;
    let current_version = ctx.version(project.as_ref())?;
    let package_name = project.package_name(root)?;
    let latest_tag = ctx.latest_tag(None, false)?;
    let all_commits = ctx.commits(latest_tag.as_deref(), false)?;

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
        if !ctx.independent_roots.is_empty() {
            data["selection"] = serde_json::json!({"project": ctx.name, "root": ctx.root, "tag_prefix": ctx.prefix});
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
        if !ctx.independent_roots.is_empty() {
            println!(
                "Release project: {} (tags: {}<version>)",
                ctx.name.as_deref().unwrap_or("(root)"),
                ctx.prefix
            );
        }
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
    preflight_for_in(&ReleaseContext::resolve(None)?, level)
}

pub(crate) fn preflight_for_in(ctx: &ReleaseContext, level: BumpLevel) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let allow_untracked = Config::load_allow_untracked(&root.join("vership.toml"))?;
    let project = project::detect(root, config.project.project_type.as_deref())?;
    let current_version = ctx.version(project.as_ref())?;
    ctx.latest_tag(None, true)?;
    let new_version = version::bump(current_version, level);
    let tag = ctx.tag(&new_version);

    let options = CheckOptions {
        expected_branch: config.project.branch.clone(),
        run_lint: config.checks.lint,
        run_tests: config.checks.tests,
        lint_command: config.checks.lint_command.clone(),
        test_command: config.checks.test_command.clone(),
        allow_uncommitted: false,
    };

    checks::run_preflight_in(
        &ctx.repo,
        root,
        &tag,
        project.as_ref(),
        &options,
        allow_untracked,
        None,
        true,
        &[],
    )?;
    eprintln!("\nAll checks passed. Ready to release.");
    Ok(())
}

/// Preview the default patch release section.
pub fn changelog_preview() -> Result<()> {
    changelog_preview_for(BumpLevel::Patch)
}

/// Preview the exact release section for the requested level.
pub fn changelog_preview_for(level: BumpLevel) -> Result<()> {
    changelog_preview_for_in(&ReleaseContext::resolve(None)?, level)
}

pub(crate) fn changelog_preview_for_in(ctx: &ReleaseContext, level: BumpLevel) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let project = project::detect(root, config.project.project_type.as_deref())?;
    let current_version = ctx.version(project.as_ref())?;
    let recovery_tag = unpublished_release_tag(ctx, &current_version)?;
    let latest_tag = match recovery_tag.as_deref() {
        Some(tag) => ctx.latest_tag(Some(tag), true)?,
        None => ctx.latest_tag(None, true)?,
    };
    let commits = ctx.commits(latest_tag.as_deref(), true)?;
    let remote_url = git::remote_url(&ctx.repo)?;

    let requested_plan = plan_bump(
        ctx,
        current_version.clone(),
        latest_tag.as_deref(),
        level,
        git::has_tracked_changes(&ctx.repo)?,
    );
    let current_tag = ctx.tag(&current_version);
    let prepared_current = recovery_tag.is_some()
        || (!git::tag_exists(&ctx.repo, &current_tag)?
            && git::ancestor_commit_has_marker(
                &ctx.repo,
                &format!("Vership-Release: {current_tag}"),
            )?);
    let next_version = if prepared_current {
        current_version
    } else {
        requested_plan.target
    };
    let generated = changelog::generate_for_tags(
        &commits,
        &next_version.to_string(),
        latest_tag.as_deref(),
        &ctx.tag(&next_version),
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;

    let existing = read_optional_text(&root.join("CHANGELOG.md"))?;
    if let Some(section) = existing.as_deref().and_then(|content| {
        if changelog::version_exists_in_changelog(content, &next_version.to_string()) {
            changelog::extract_section(content, &next_version.to_string())
        } else {
            None
        }
    }) {
        println!("{section}");
        return Ok(());
    }

    let curated_policy = Config::load_curated_policy(&root.join("vership.toml"))?;
    let noted = noted_generated_entries(
        ctx,
        &commits,
        latest_tag.as_deref(),
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )?;
    let update = changelog::integrate_changelog_with_coverage(
        existing.as_deref(),
        &generated,
        curated_policy,
        &noted,
    )
    .map_err(Error::CheckFailed)?;
    if update.promoted {
        report_curated_promotion("Previewing", &update, curated_policy);
    }
    let changelog_section = changelog::extract_section(&update.content, &next_version.to_string())
        .unwrap_or(&generated);

    println!("{changelog_section}");
    Ok(())
}

/// The generated entries already covered because their own commit added a
/// curated note under `## [Unreleased]`, rendered exactly as the generated
/// section renders them so the merge can match them by content.
fn noted_generated_entries(
    ctx: &ReleaseContext,
    commits: &[git::Commit],
    previous_tag: Option<&str>,
    remote_url: Option<&str>,
    unconventional_mode: &str,
) -> Result<Vec<String>> {
    let changelog = ctx.repo_path(Path::new("CHANGELOG.md"))?;
    let noted_hashes = git::commits_adding_unreleased_notes_at(
        &ctx.repo,
        previous_tag,
        &changelog.to_string_lossy(),
    )?;
    let noted_commits: Vec<git::Commit> = commits
        .iter()
        .filter(|commit| noted_hashes.contains(&commit.hash))
        .cloned()
        .collect();
    Ok(changelog::generated_entries_for(
        &noted_commits,
        remote_url,
        unconventional_mode,
    ))
}

/// Say what promoting the curated `## [Unreleased]` notes did to the entries
/// generated from commits, entry by entry, so nothing leaves or enters the
/// release unseen.
fn report_curated_promotion(verb: &str, update: &ChangelogIntegration, policy: CuratedPolicy) {
    fn counted(count: usize, singular: &str, plural: &str) -> String {
        format!("{count} {}", if count == 1 { singular } else { plural })
    }
    match policy {
        CuratedPolicy::Merge => {
            let mut summary = format!(
                "{verb} curated Unreleased notes ({} merged",
                counted(
                    update.merged_entries.len(),
                    "generated entry",
                    "generated entries"
                )
            );
            if !update.cited_entries.is_empty() {
                summary.push_str(&format!(
                    ", {} already cited",
                    counted(update.cited_entries.len(), "entry", "entries")
                ));
            }
            if !update.noted_entries.is_empty() {
                summary.push_str(&format!(
                    ", {} noted by its own commit",
                    counted(update.noted_entries.len(), "entry", "entries")
                ));
            }
            summary.push(')');
            output::print_step(&summary);
            for entry in &update.merged_entries {
                eprintln!("    merged: {}", changelog::entry_summary(entry));
            }
            for entry in &update.cited_entries {
                eprintln!("    cited:  {}", changelog::entry_summary(entry));
            }
            for entry in &update.noted_entries {
                eprintln!("    noted:  {}", changelog::entry_summary(entry));
            }
        }
        CuratedPolicy::Replace => {
            output::print_step(&format!(
                "{verb} curated Unreleased notes ({} replaced, changelog.curated = \"replace\")",
                counted(
                    update.omitted_entries.len(),
                    "generated entry",
                    "generated entries"
                )
            ));
            for entry in &update.omitted_entries {
                eprintln!("    replaced: {}", changelog::entry_summary(entry));
            }
        }
    }
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
    bump_with_prepare_in(
        &ReleaseContext::resolve(None)?,
        level,
        dry_run,
        skip_checks,
        no_push,
        prepare,
    )
}

pub(crate) fn bump_with_prepare_in(
    ctx: &ReleaseContext,
    level: BumpLevel,
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let project = project::detect(root, config.project.project_type.as_deref())?;

    let on_disk = ctx.version(project.as_ref())?;
    let recovery_tag = unpublished_release_tag(ctx, &on_disk)?;
    let current_tag = ctx.tag(&on_disk);
    if recovery_tag.is_none()
        && !git::tag_exists(&ctx.repo, &current_tag)?
        && git::ancestor_commit_has_marker(&ctx.repo, &format!("Vership-Release: {current_tag}"))?
    {
        return Err(Error::CheckFailed(format!(
            "{current_tag} is already prepared; use release to finish it, or release --prepare to refresh it; do not bump again"
        )));
    }

    let latest_tag = match recovery_tag.as_deref() {
        Some(tag) => ctx.latest_tag(Some(tag), true)?,
        None => ctx.latest_tag(None, true)?,
    };
    // Resume detection is based on tracked release files only. An unrelated
    // untracked path must never turn a fresh bump into an interrupted release.
    let has_uncommitted = git::has_tracked_changes(&ctx.repo)?;

    let plan = if recovery_tag.is_some() {
        plan_current(ctx, on_disk, latest_tag.as_deref())?
    } else {
        plan_bump(ctx, on_disk, latest_tag.as_deref(), level, has_uncommitted)
    };
    execute(
        ctx,
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
        prepare,
        recovery_tag,
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
    release_current_with_prepare_in(
        &ReleaseContext::resolve(None)?,
        dry_run,
        skip_checks,
        no_push,
        prepare,
    )
}

pub(crate) fn release_current_with_prepare_in(
    ctx: &ReleaseContext,
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let project = project::detect(root, config.project.project_type.as_deref())?;

    let on_disk = ctx.version(project.as_ref())?;
    let recovery_tag = unpublished_release_tag(ctx, &on_disk)?;
    let latest_tag = match recovery_tag.as_deref() {
        Some(tag) => ctx.latest_tag(Some(tag), true)?,
        None => ctx.latest_tag(None, true)?,
    };

    let plan = plan_current(ctx, on_disk, latest_tag.as_deref())?;
    execute(
        ctx,
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
        prepare,
        recovery_tag,
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
    resume_with_prepare_in(
        &ReleaseContext::resolve(None)?,
        dry_run,
        skip_checks,
        no_push,
        prepare,
    )
}

pub(crate) fn resume_with_prepare_in(
    ctx: &ReleaseContext,
    dry_run: bool,
    skip_checks: bool,
    no_push: bool,
    prepare: bool,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let project = project::detect(root, config.project.project_type.as_deref())?;

    let on_disk = ctx.version(project.as_ref())?;
    let recovery_tag = unpublished_release_tag(ctx, &on_disk)?;
    let latest_tag = match recovery_tag.as_deref() {
        Some(tag) => ctx.latest_tag(Some(tag), true)?,
        None => ctx.latest_tag(None, true)?,
    };

    let plan = plan_resume(ctx, on_disk, latest_tag.as_deref())?;
    execute(
        ctx,
        plan,
        ExecOpts {
            dry_run,
            skip_checks,
            no_push,
        },
        prepare,
        recovery_tag,
    )
}

/// Single linear orchestrator. Runs preflight, optionally writes the version,
/// generates changelog, commits, tags, and pushes.
fn execute(
    ctx: &ReleaseContext,
    plan: ReleasePlan,
    opts: ExecOpts,
    prepare: bool,
    recovery_tag: Option<String>,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let allow_untracked = Config::load_allow_untracked(&root.join("vership.toml"))?;
    let project = project::detect(root, config.project.project_type.as_deref())?;
    let tag = ctx.tag(&plan.target);

    if plan.allow_dirty_tree {
        output::print_step(&format!(
            "Resuming interrupted release: target {}",
            plan.target
        ));
    }

    let initial_head = git::head_oid(&ctx.repo)?;
    let initial_untracked = git::untracked_files(&ctx.repo)?;
    let expected = expected_release_paths(ctx, project.as_ref())?;
    if plan.allow_dirty_tree {
        validate_release_changes(ctx, &expected, &initial_untracked)?;
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
    checks::run_preflight_in(
        &ctx.repo,
        root,
        &tag,
        project.as_ref(),
        &check_options,
        allow_untracked,
        recovery_tag.as_deref(),
        true,
        if plan.allow_dirty_tree {
            &expected
        } else {
            &[]
        },
    )?;

    if let Some(previous) = plan
        .previous_tag
        .as_deref()
        .and_then(|tag| ctx.tag_version(tag))
        && !plan.target.cmp_precedence(&previous).is_gt()
    {
        return Err(Error::Version(format!(
            "target {} must have greater semantic precedence than {previous}",
            plan.target
        )));
    }

    if git::head_oid(&ctx.repo)? != initial_head {
        return Err(Error::CheckFailed(
            "HEAD changed during release checks; inspect the repository before retrying".into(),
        ));
    }

    let changelog_path = root.join("CHANGELOG.md");
    let mut prepared_existing = read_optional_text(&changelog_path)?;
    let release_marker = format!("Vership-Release: {tag}");
    let history_has_release_marker = git::ancestor_commit_has_marker(&ctx.repo, &release_marker)?;
    let release_work_already_done = plan.mutation == Mutation::None
        && !git::has_tracked_changes(&ctx.repo)?
        && (history_has_release_marker || recovery_tag.as_deref() == Some(tag.as_str()))
        && prepared_existing.as_deref().is_some_and(|content| {
            changelog::version_exists_in_changelog(content, &plan.target.to_string())
        });

    if history_has_release_marker && !release_work_already_done {
        return Err(Error::CheckFailed(format!(
            "prepared release {tag} is incomplete or changed: restore its version section and clean release files before refreshing; bump hooks were not replayed"
        )));
    }
    if release_work_already_done {
        let prepared_oid = git::prepared_oid(&ctx.repo, &tag)?;
        let head = git::head_oid(&ctx.repo)?;
        if prepared_oid.as_deref() != Some(head.as_str()) {
            if !prepare {
                return Err(Error::CheckFailed(format!(
                    "HEAD advanced or preparation identity is missing for {tag}; review current content and run `vership{} release --prepare` to refresh preparation",
                    ctx.name
                        .as_ref()
                        .map(|n| format!(" --project {n}"))
                        .unwrap_or_default()
                )));
            }
            if let Some(oid) = prepared_oid
                && let Some(refreshed) = refresh_changelog(ctx, &plan, &oid, opts.dry_run)?
            {
                prepared_existing = Some(refreshed);
            }
            if !opts.dry_run {
                git::commit_preparation(&ctx.repo, &tag)?;
                git::record_preparation(&ctx.repo, &tag)?;
            }
        }
    }
    if recovery_tag.is_some() && !release_work_already_done {
        return Err(Error::CheckFailed(format!(
            "local tag {tag} is marked as an unpublished Vership release, but the clean release state cannot be verified; leave the tag in place and inspect HEAD, the version, and CHANGELOG.md"
        )));
    }

    // Release commits carry an explicit marker so a manually committed version
    // and changelog can never be mistaken for `--prepare`. For marked commits,
    // do not replay bump hooks or artifact generators: they may have external
    // or non-idempotent side effects. Only the tag/push phase remains.
    if release_work_already_done {
        output::print_step("Nothing to commit (release commit already exists)");
        if opts.dry_run {
            eprintln!("\n--- Dry run: no changes made ---");
            if let Some(preview) = prepared_existing
                .as_deref()
                .and_then(|content| changelog::extract_section(content, &plan.target.to_string()))
            {
                eprintln!("\nChangelog preview:\n");
                eprintln!("{preview}");
            }
            return Ok(());
        }
        if recovery_tag.is_some() {
            git::delete_local_tag(&ctx.repo, &tag)?;
            output::print_step(&format!("Recovering unpublished local tag: {tag}"));
        }
        return finish_release(ctx, &tag, opts.no_push, prepare);
    }

    // Pre-bump hook
    if !opts.dry_run {
        hooks::run_hook(root, "pre-bump", config.hooks.pre_bump.as_deref())?;
    }

    // Hooks are allowed to curate the changelog. Read it only after pre-bump
    // has completed so those edits become the source for integration.
    let existing = read_optional_text(&changelog_path)?;

    // Hooks may create files or symlinks. Recheck every manifest destination.
    expected_release_paths(ctx, project.as_ref())?;

    // Mutation: write version into manifest + apply version_files (when planned).
    let on_disk = ctx.version(project.as_ref())?;
    let vf_touched = match plan.mutation {
        Mutation::Bump => {
            output::print_step(&format!("Bumping {on_disk} → {}", plan.target));
            if !opts.dry_run {
                project.write_version(root, &plan.target)?;
                project.sync_lockfile(root)?;
            }
            output::print_step(&format!("Updated {}", project.name().to_lowercase()));

            if !opts.dry_run && !config.version_files.is_empty() {
                output::print_step("Updating version files");
                version_files::apply(
                    root,
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
                project.write_version(root, &plan.target)?;
                project.sync_lockfile(root)?;
                if !config.version_files.is_empty() {
                    output::print_step("Updating version files");
                    let previous = plan
                        .previous_tag
                        .as_deref()
                        .and_then(|tag| ctx.tag_version(tag))
                        .unwrap_or_else(|| plan.target.clone());
                    let mut touched = version_files::apply(
                        root,
                        &config.version_files,
                        &previous.to_string(),
                        &plan.target.to_string(),
                    )?;
                    touched.extend(version_files::apply(
                        root,
                        &config.version_files,
                        &plan.target.to_string(),
                        &plan.target.to_string(),
                    )?);
                    touched.sort();
                    touched.dedup();
                    touched
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
    let commits = ctx.commits(plan.previous_tag.as_deref(), true)?;
    let remote_url = git::remote_url(&ctx.repo)?;

    let changelog_section = changelog::generate_for_tags(
        &commits,
        &plan.target.to_string(),
        plan.previous_tag.as_deref(),
        &tag,
        remote_url.as_deref(),
        &config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;

    // Guard against duplicate sections when resuming after changelog was written.
    let changelog_already_written = existing
        .as_deref()
        .is_some_and(|c| changelog::version_exists_in_changelog(c, &plan.target.to_string()));
    let curated_policy = Config::load_curated_policy(&root.join("vership.toml"))?;
    let integration = if changelog_already_written {
        None
    } else {
        let noted = noted_generated_entries(
            ctx,
            &commits,
            plan.previous_tag.as_deref(),
            remote_url.as_deref(),
            &config.changelog.unconventional,
        )?;
        Some(
            changelog::integrate_changelog_with_coverage(
                existing.as_deref(),
                &changelog_section,
                curated_policy,
                &noted,
            )
            .map_err(Error::CheckFailed)?,
        )
    };
    let full_changelog = match &integration {
        Some(update) => update.content.clone(),
        None => existing.clone().unwrap_or_default(),
    };

    let entry_count = commits
        .iter()
        .filter(|commit| {
            if commit.message.starts_with("Merge ") {
                return false;
            }
            match changelog::parse_conventional_commit(&commit.message) {
                Some(cc) => {
                    cc.breaking
                        || matches!(cc.commit_type.as_str(), "feat" | "fix" | "perf" | "change")
                }
                None => config.changelog.unconventional == "include",
            }
        })
        .count();
    if changelog_already_written {
        output::print_step(&format!(
            "Changelog already up-to-date ({entry_count} entries)"
        ));
    } else if let Some(update) = integration.as_ref().filter(|update| update.promoted) {
        report_curated_promotion("Promoted", update, curated_policy);
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
        let excluded: Vec<_> = ctx
            .independent_roots
            .iter()
            .filter(|p| **p != ctx.root)
            .cloned()
            .collect();
        artifacts::run_scoped(root, &config.artifacts, &excluded)?
    } else {
        Vec::new()
    };

    hooks::run_hook(root, "post-bump", config.hooks.post_bump.as_deref())?;

    if !project.is_tag_versioned() && project.read_version(root)? != plan.target {
        return Err(Error::CheckFailed("release hooks changed the selected manifest away from the target version; changes preserved".into()));
    }
    let final_notes = std::fs::read_to_string(&changelog_path)?;
    if !changelog::version_exists_in_changelog(&final_notes, &plan.target.to_string()) {
        return Err(Error::CheckFailed(
            "release hooks removed the target changelog section; changes preserved".into(),
        ));
    }

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
    let stage_paths: Vec<_> = stage_refs
        .iter()
        .map(|p| ctx.repo_path(Path::new(p)))
        .collect::<Result<_>>()?;
    if git::head_oid(&ctx.repo)? != initial_head {
        return Err(Error::CheckFailed("HEAD changed during release checks or hooks; inspect the preserved changes before retrying".into()));
    }
    validate_release_changes(ctx, &stage_paths, &initial_untracked)?;
    let stage_paths: Vec<_> = stage_paths
        .into_iter()
        .filter(|path| {
            ctx.repo.join(path).exists()
                || git::tracked_path(&ctx.repo, &path.to_string_lossy()).unwrap_or(false)
        })
        .collect();
    let stage_strings: Vec<_> = stage_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    git::stage_files(
        &ctx.repo,
        &stage_strings.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;

    // A resumed or retagged release may have nothing left to commit: the
    // version bump and changelog already landed in a prior run. Skip the
    // empty commit and proceed to the tag, which is the step that remains.
    if git::has_staged_changes(&ctx.repo)? {
        let subject = if project.is_tag_versioned() {
            format!("chore: release {tag}")
        } else {
            format!("chore: bump version to {tag}")
        };
        let commit_msg = format!("{subject}\n\n{release_marker}");
        git::commit(&ctx.repo, &commit_msg)?;
        output::print_step(&format!("Committed: {subject}"));
    } else {
        output::print_step("Nothing to commit (release commit already exists)");
    }

    git::record_preparation(&ctx.repo, &tag)?;
    finish_release(ctx, &tag, opts.no_push, prepare)
}

fn finish_release(ctx: &ReleaseContext, tag: &str, no_push: bool, prepare: bool) -> Result<()> {
    let root = &ctx.repo;
    let config = &ctx.config;
    if prepare {
        output::print_step(&format!(
            "Prepared release commit for {tag}; review it, then run `vership{} release` to tag and push",
            ctx.name
                .as_ref()
                .map(|n| format!(" --project {n}"))
                .unwrap_or_default()
        ));
        return Ok(());
    }

    if git::has_tracked_changes(root)? {
        return Err(Error::CheckFailed(
            "working tree changed before tagging; inspect the preserved changes".into(),
        ));
    }
    git::create_tag(root, tag)?;
    output::print_step(&format!("Tagged: {tag}"));

    if no_push {
        let branch = git::current_branch(root)?;
        output::print_step(&format!("Ready to push: git push origin {branch} {tag}"));
        return Ok(());
    }

    let before_hook = git::untracked_files(root)?;
    hooks::run_hook(&ctx.root, "pre-push", config.hooks.pre_push.as_deref())?;
    validate_release_changes(ctx, &[], &before_hook)?;
    if !git::tag_points_to_head(root, tag)? {
        return Err(Error::CheckFailed(
            "HEAD changed during pre-push hook; tag preserved, nothing pushed".into(),
        ));
    }

    let branch = git::current_branch(root)?;
    for pending in git::outgoing_release_tags(root, &branch)? {
        if pending != tag && !git::remote_tag_exists(root, &pending)? {
            return Err(Error::CheckFailed(format!(
                "push would publish another unfinished release ({pending}); resolve the other preparation before publishing; nothing pushed"
            )));
        }
    }
    git::push_with_tag(root, &branch, tag)?;
    output::print_step("Pushed to origin");

    hooks::run_hook(&ctx.root, "post-push", config.hooks.post_push.as_deref())?;

    Ok(())
}

fn normalized_tag(ctx: &ReleaseContext, tag: Option<&str>) -> Option<String> {
    tag.and_then(|t| ctx.tag_version(t))
        .map(|v| format!("v{v}"))
}
fn plan_bump(
    ctx: &ReleaseContext,
    version: semver::Version,
    previous: Option<&str>,
    level: BumpLevel,
    dirty: bool,
) -> ReleasePlan {
    let mut plan = ReleasePlan::bump(
        version,
        normalized_tag(ctx, previous).as_deref(),
        level,
        dirty,
    );
    plan.previous_tag = previous.map(str::to_owned);
    plan
}
fn plan_current(
    ctx: &ReleaseContext,
    version: semver::Version,
    previous: Option<&str>,
) -> Result<ReleasePlan> {
    if let Some(previous_version) = previous.and_then(|tag| ctx.tag_version(tag))
        && !version.cmp_precedence(&previous_version).is_gt()
    {
        return Err(Error::Version(format!(
            "version {version} must have greater semantic precedence than {previous_version}"
        )));
    }
    let mut plan = ReleasePlan::release_current(version, normalized_tag(ctx, previous).as_deref())?;
    plan.previous_tag = previous.map(str::to_owned);
    Ok(plan)
}
fn plan_resume(
    ctx: &ReleaseContext,
    version: semver::Version,
    previous: Option<&str>,
) -> Result<ReleasePlan> {
    let mut plan = ReleasePlan::resume(version, normalized_tag(ctx, previous).as_deref())?;
    plan.previous_tag = previous.map(str::to_owned);
    Ok(plan)
}

fn expected_release_paths(
    ctx: &ReleaseContext,
    project: &dyn project::ProjectType,
) -> Result<Vec<std::path::PathBuf>> {
    let mut paths = project.release_files(&ctx.root)?;
    paths.push("CHANGELOG.md".into());
    for entry in &ctx.config.version_files {
        for path in glob::glob(&ctx.root.join(&entry.glob).to_string_lossy())
            .map_err(|e| Error::Config(e.to_string()))?
        {
            let path = path.map_err(|e| Error::Config(e.to_string()))?;
            paths.push(
                path.strip_prefix(&ctx.root)
                    .map_err(|e| Error::Config(e.to_string()))?
                    .to_path_buf(),
            );
        }
    }
    for entry in &ctx.config.artifacts {
        paths.extend(
            entry
                .output
                .iter()
                .chain(entry.files.iter())
                .map(std::path::PathBuf::from),
        );
    }
    paths
        .iter()
        .map(|p| {
            let relative = ctx.repo_path(p)?;
            let physical = ReleaseContext::physical_path(&ctx.repo.join(&relative))?;
            if ctx
                .independent_roots
                .iter()
                .any(|root| root != &ctx.root && physical.starts_with(root))
            {
                return Err(Error::Config(format!(
                    "release file {} belongs to another registered project",
                    relative.display()
                )));
            }
            Ok(relative)
        })
        .collect()
}

fn validate_release_changes(
    ctx: &ReleaseContext,
    expected: &[std::path::PathBuf],
    initial_untracked: &[String],
) -> Result<()> {
    let mut changed = git::changed_paths(&ctx.repo)?;
    changed.extend(
        git::untracked_files(&ctx.repo)?
            .into_iter()
            .filter(|p| !initial_untracked.contains(p))
            .map(std::path::PathBuf::from),
    );
    if let Some(path) = changed.iter().find(|p| !expected.contains(p)) {
        return Err(Error::CheckFailed(format!(
            "unexpected release change in {}; only the selected project's declared release files may be committed; your changes were preserved",
            path.display()
        )));
    }
    for path in expected {
        ReleaseContext::contained(&ctx.repo, path)?;
    }
    Ok(())
}

/// Runtime dispatch. Public library entry points retain their original signatures.
#[doc(hidden)]
pub fn run_project(
    command: crate::cli::RuntimeCommand,
    selector: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    use crate::cli::{ConfigCommand, RuntimeCommand as C};
    let ctx = ReleaseContext::resolve(selector)?;
    match command {
        C::Status {
            limit,
            offset,
            fields,
        } => status_in(&ctx, output, limit, offset, fields.as_deref()),
        C::Preflight { level } => preflight_for_in(&ctx, level),
        C::Changelog { level } => changelog_preview_for_in(&ctx, level),
        C::Bump {
            level,
            dry_run,
            skip_checks,
            no_push,
            prepare,
        } => bump_with_prepare_in(&ctx, level, dry_run, skip_checks, no_push, prepare),
        C::Release {
            dry_run,
            skip_checks,
            no_push,
            prepare,
        } => release_current_with_prepare_in(&ctx, dry_run, skip_checks, no_push, prepare),
        C::Resume {
            dry_run,
            skip_checks,
            no_push,
            prepare,
        } => resume_with_prepare_in(&ctx, dry_run, skip_checks, no_push, prepare),
        C::Verify {
            version,
            targets,
            skip,
        } => crate::verify::run_in(
            &ctx,
            version.as_deref(),
            targets.as_deref(),
            skip.as_deref(),
            output,
        ),
        C::UpdateLocal {
            version,
            managers,
            skip,
            dry_run,
        } => crate::update_local::run_in(
            &ctx,
            version.as_deref(),
            managers.as_deref(),
            skip.as_deref(),
            dry_run,
            output,
        ),
        C::Config(ConfigCommand::Show) => crate::config::show_in(&ctx, output),
        _ => Err(Error::Config(
            "--project is not supported for this command".into(),
        )),
    }
}

/// Refresh notes for commits added after preparation without re-running bump
/// hooks or artifact commands. Existing released notes remain curated content.
fn refresh_changelog(
    ctx: &ReleaseContext,
    plan: &ReleasePlan,
    oid: &str,
    dry_run: bool,
) -> Result<Option<String>> {
    let commits = ctx.commits(Some(oid), true)?;
    let remote = git::remote_url(&ctx.repo)?;
    let generated = changelog::generate_for_tags(
        &commits,
        &plan.target.to_string(),
        plan.previous_tag.as_deref(),
        &ctx.tag(&plan.target),
        remote.as_deref(),
        &ctx.config.changelog.unconventional,
    )
    .map_err(Error::CheckFailed)?;
    let path = ctx.root.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&path)?;
    let section = changelog::extract_section(&existing, &plan.target.to_string())
        .ok_or_else(|| Error::CheckFailed("prepared version section is missing".into()))?;
    let body = section.split_once('\n').map(|(_, body)| body).unwrap_or("");
    let curated = format!("# Changelog\n\n## [Unreleased]\n\n{body}");
    let merged = changelog::integrate_changelog_with_coverage(
        Some(&curated),
        &generated,
        CuratedPolicy::Merge,
        &[],
    )
    .map_err(Error::CheckFailed)?;
    let refreshed = changelog::extract_section(&merged.content, &plan.target.to_string())
        .ok_or_else(|| Error::Other("refresh did not produce a release section".into()))?;
    if section == refreshed {
        return Ok(None);
    }
    let updated = existing.replacen(section, refreshed, 1);
    if !dry_run {
        ReleaseContext::contained(&ctx.root, Path::new("CHANGELOG.md"))?;
        std::fs::write(path, &updated)?;
        let relative = ctx.repo_path(Path::new("CHANGELOG.md"))?;
        git::stage_files(&ctx.repo, &[&relative.to_string_lossy()])?;
    }
    output::print_step("Refreshed prepared notes for commits added since preparation");
    Ok(Some(updated))
}
