//! Pure planning logic for releases. No I/O.
//!
//! A `ReleasePlan` captures the answers to two orthogonal questions:
//! - What version are we tagging? (`target`)
//! - Do we need to write that version into the manifest? (`mutation`)
//!
//! Plus one practical concern: do we expect the working tree to be clean
//! before we start? (`allow_dirty_tree`) — only `false` for resume cases,
//! where an interrupted prior run left the manifest already mutated.
//!
//! The orchestrator in `release::execute` consumes a plan and runs the
//! same linear flow regardless of how it was constructed.

use semver::Version;

use crate::cli::BumpLevel;
use crate::error::{Error, Result};
use crate::version;

/// Whether the manifest needs writing.
#[derive(Debug, Eq, PartialEq)]
pub enum Mutation {
    /// Write `target` into the manifest and apply configured `version_files`.
    Bump,
    /// Manifest is already at `target`. Skip writes.
    None,
}

/// A complete description of a release, derived purely from inputs.
#[derive(Debug)]
pub struct ReleasePlan {
    /// Most recent released tag (e.g. `"v0.1.0"`), or `None` for the first release.
    /// Used for changelog scope and the previous-version compare link.
    pub previous_tag: Option<String>,
    /// Version to tag.
    pub target: Version,
    /// Whether to write `target` into the manifest.
    pub mutation: Mutation,
    /// Whether to permit uncommitted changes during preflight.
    /// True only when resuming an interrupted bump.
    pub allow_dirty_tree: bool,
}

impl ReleasePlan {
    /// Tag string in `vX.Y.Z` form.
    pub fn tag(&self) -> String {
        format!("v{}", self.target)
    }

    /// Standard bump from the latest released tag.
    ///
    /// Auto-detects an interrupted prior run: if the on-disk version already
    /// equals what bumping `latest_tag` by `level` would produce, AND the
    /// working tree has uncommitted changes, the plan describes a resume
    /// (no further mutation, dirty tree allowed) instead of a fresh bump.
    pub fn bump(
        on_disk: Version,
        latest_tag: Option<&str>,
        level: BumpLevel,
        has_uncommitted: bool,
    ) -> Self {
        if let Some(tag_str) = latest_tag
            && let Ok(tag_version) = Version::parse(tag_str.trim_start_matches('v'))
            && version::bump(tag_version.clone(), level) == on_disk
            && has_uncommitted
        {
            return Self {
                previous_tag: latest_tag.map(String::from),
                target: on_disk,
                mutation: Mutation::None,
                allow_dirty_tree: true,
            };
        }
        let target = version::bump(on_disk.clone(), level);
        Self {
            previous_tag: latest_tag.map(String::from),
            target,
            mutation: Mutation::Bump,
            allow_dirty_tree: false,
        }
    }

    /// Tag the on-disk version as-is. Used for initial releases or when the
    /// version was set manually.
    ///
    /// Errors if the on-disk version is not strictly greater than the latest
    /// released tag.
    pub fn release_current(on_disk: Version, latest_tag: Option<&str>) -> Result<Self> {
        if let Some(prev) = parse_tag(latest_tag)?
            && on_disk <= prev
        {
            return Err(Error::Version(format!(
                "on-disk version {on_disk} is not greater than latest tag v{prev}; \
                 use `vership bump` to increment"
            )));
        }
        Ok(Self {
            previous_tag: latest_tag.map(String::from),
            target: on_disk,
            mutation: Mutation::None,
            allow_dirty_tree: false,
        })
    }

    /// Resume an interrupted bump: trust the on-disk version and finish
    /// committing/tagging/pushing.
    ///
    /// Errors if the on-disk version is not strictly greater than the latest
    /// released tag — there's nothing to resume in that case.
    pub fn resume(on_disk: Version, latest_tag: Option<&str>) -> Result<Self> {
        if let Some(prev) = parse_tag(latest_tag)?
            && on_disk <= prev
        {
            return Err(Error::Version(format!(
                "on-disk version {on_disk} is not greater than latest tag v{prev}; \
                 nothing to resume"
            )));
        }
        Ok(Self {
            previous_tag: latest_tag.map(String::from),
            target: on_disk,
            mutation: Mutation::None,
            allow_dirty_tree: true,
        })
    }
}

fn parse_tag(tag: Option<&str>) -> Result<Option<Version>> {
    let Some(s) = tag else {
        return Ok(None);
    };
    Version::parse(s.trim_start_matches('v'))
        .map(Some)
        .map_err(|e| Error::Version(format!("invalid latest tag '{s}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    // ---- bump ----

    #[test]
    fn bump_normal_patch() {
        let plan = ReleasePlan::bump(v("0.1.0"), Some("v0.1.0"), BumpLevel::Patch, false);
        assert_eq!(plan.target, v("0.1.1"));
        assert_eq!(plan.mutation, Mutation::Bump);
        assert!(!plan.allow_dirty_tree);
        assert_eq!(plan.previous_tag.as_deref(), Some("v0.1.0"));
    }

    #[test]
    fn bump_normal_minor() {
        let plan = ReleasePlan::bump(v("0.1.5"), Some("v0.1.5"), BumpLevel::Minor, false);
        assert_eq!(plan.target, v("0.2.0"));
        assert_eq!(plan.mutation, Mutation::Bump);
    }

    #[test]
    fn bump_normal_major() {
        let plan = ReleasePlan::bump(v("0.9.0"), Some("v0.9.0"), BumpLevel::Major, false);
        assert_eq!(plan.target, v("1.0.0"));
        assert_eq!(plan.mutation, Mutation::Bump);
    }

    #[test]
    fn bump_first_release_no_tag() {
        let plan = ReleasePlan::bump(v("0.1.0"), None, BumpLevel::Patch, false);
        assert_eq!(plan.target, v("0.1.1"));
        assert_eq!(plan.mutation, Mutation::Bump);
        assert_eq!(plan.previous_tag, None);
    }

    #[test]
    fn bump_auto_resume_when_already_bumped_and_dirty() {
        // Prior run bumped 0.1.70 → 0.1.71 in the manifest but didn't commit.
        let plan = ReleasePlan::bump(v("0.1.71"), Some("v0.1.70"), BumpLevel::Patch, true);
        assert_eq!(plan.target, v("0.1.71"));
        assert_eq!(plan.mutation, Mutation::None);
        assert!(plan.allow_dirty_tree);
    }

    #[test]
    fn bump_no_resume_when_clean_tree() {
        // On-disk matches what bump would produce, but the tree is clean —
        // this is a normal already-released state, not an interrupted run.
        // Treat as a normal bump (which will fail later: tag exists).
        let plan = ReleasePlan::bump(v("0.1.71"), Some("v0.1.70"), BumpLevel::Patch, false);
        assert_eq!(plan.target, v("0.1.72"));
        assert_eq!(plan.mutation, Mutation::Bump);
    }

    #[test]
    fn bump_no_resume_when_wrong_level() {
        // Files bumped to minor (0.2.0) but caller asks for patch.
        let plan = ReleasePlan::bump(v("0.2.0"), Some("v0.1.70"), BumpLevel::Patch, true);
        assert_eq!(plan.target, v("0.2.1"));
        assert_eq!(plan.mutation, Mutation::Bump);
    }

    #[test]
    fn bump_auto_resume_minor() {
        let plan = ReleasePlan::bump(v("0.2.0"), Some("v0.1.70"), BumpLevel::Minor, true);
        assert_eq!(plan.target, v("0.2.0"));
        assert_eq!(plan.mutation, Mutation::None);
        assert!(plan.allow_dirty_tree);
    }

    #[test]
    fn bump_auto_resume_major() {
        let plan = ReleasePlan::bump(v("1.0.0"), Some("v0.9.5"), BumpLevel::Major, true);
        assert_eq!(plan.target, v("1.0.0"));
        assert_eq!(plan.mutation, Mutation::None);
    }

    // ---- release_current ----

    #[test]
    fn release_current_initial_release_no_tag() {
        let plan = ReleasePlan::release_current(v("0.1.0"), None).unwrap();
        assert_eq!(plan.target, v("0.1.0"));
        assert_eq!(plan.mutation, Mutation::None);
        assert!(!plan.allow_dirty_tree);
        assert_eq!(plan.previous_tag, None);
    }

    #[test]
    fn release_current_after_manual_edit() {
        // User edited Cargo.toml from 0.1.0 to 0.2.0 manually.
        let plan = ReleasePlan::release_current(v("0.2.0"), Some("v0.1.0")).unwrap();
        assert_eq!(plan.target, v("0.2.0"));
        assert_eq!(plan.mutation, Mutation::None);
        assert_eq!(plan.previous_tag.as_deref(), Some("v0.1.0"));
    }

    #[test]
    fn release_current_rejects_equal_to_latest_tag() {
        let err = ReleasePlan::release_current(v("0.1.0"), Some("v0.1.0")).unwrap_err();
        assert!(matches!(err, Error::Version(_)), "got {err:?}");
    }

    #[test]
    fn release_current_rejects_below_latest_tag() {
        let err = ReleasePlan::release_current(v("0.0.9"), Some("v0.1.0")).unwrap_err();
        assert!(matches!(err, Error::Version(_)), "got {err:?}");
    }

    #[test]
    fn release_current_rejects_invalid_tag() {
        let err = ReleasePlan::release_current(v("0.1.0"), Some("not-a-version")).unwrap_err();
        assert!(matches!(err, Error::Version(_)), "got {err:?}");
    }

    // ---- resume ----

    #[test]
    fn resume_after_interrupted_bump() {
        let plan = ReleasePlan::resume(v("0.1.71"), Some("v0.1.70")).unwrap();
        assert_eq!(plan.target, v("0.1.71"));
        assert_eq!(plan.mutation, Mutation::None);
        assert!(plan.allow_dirty_tree);
    }

    #[test]
    fn resume_works_without_prior_tag() {
        // Edge case: first release was interrupted before tagging.
        let plan = ReleasePlan::resume(v("0.1.0"), None).unwrap();
        assert_eq!(plan.target, v("0.1.0"));
        assert_eq!(plan.mutation, Mutation::None);
        assert!(plan.allow_dirty_tree);
    }

    #[test]
    fn resume_rejects_equal_to_latest_tag() {
        let err = ReleasePlan::resume(v("0.1.0"), Some("v0.1.0")).unwrap_err();
        assert!(matches!(err, Error::Version(_)), "got {err:?}");
    }

    #[test]
    fn resume_rejects_below_latest_tag() {
        let err = ReleasePlan::resume(v("0.0.9"), Some("v0.1.0")).unwrap_err();
        assert!(matches!(err, Error::Version(_)), "got {err:?}");
    }

    // ---- tag formatting ----

    #[test]
    fn tag_prepends_v() {
        let plan = ReleasePlan::release_current(v("1.2.3"), None).unwrap();
        assert_eq!(plan.tag(), "v1.2.3");
    }
}
