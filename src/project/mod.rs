pub mod ansible;
pub mod cargo_helpers;
pub mod detect;
pub mod go;
pub mod gradle;
pub mod node;
pub mod python;
pub mod rust;
pub mod rust_maturin;
pub mod workspace_deps;

use std::path::{Path, PathBuf};

use crate::error::Result;

pub trait ProjectType {
    /// Display name (e.g. "Rust", "Rust + Maturin")
    fn name(&self) -> &str;

    /// Read current version from project files
    fn read_version(&self, root: &Path) -> Result<semver::Version>;

    /// Write new version to all relevant files
    fn write_version(&self, root: &Path, version: &semver::Version) -> Result<()>;

    /// Verify lock files are in sync
    fn verify_lockfile(&self, root: &Path) -> Result<()>;

    /// Sync lock files after version bump
    fn sync_lockfile(&self, root: &Path) -> Result<()>;

    /// Run lint checks
    fn run_lint(&self, root: &Path) -> Result<()>;

    /// Run tests
    fn run_tests(&self, root: &Path) -> Result<()>;

    /// Files that were modified by write_version
    fn modified_files(&self) -> Vec<PathBuf>;

    /// Human-facing package identity (e.g. an Ansible collection FQCN
    /// `namespace.name`). Surfaced in `status`. Defaults to `None` for project
    /// types whose package name is not meaningful to report.
    fn package_name(&self, _root: &Path) -> Result<Option<String>> {
        Ok(None)
    }

    /// Whether the version source is the git tag rather than a project file.
    /// When true, release uses "chore: release" instead of "chore: bump version to".
    fn is_tag_versioned(&self) -> bool {
        false
    }

    /// Whether the only published artifact for a release of this project type is
    /// the git tag itself. Ansible collections are consumed directly by git ref
    /// (`git+<url>,v<version>`), so pushing the tag is the entire release: there
    /// is no GitHub Release, crate, wheel, or npm package to verify. `verify`
    /// uses this to check the tag alone, ignoring incidental package metadata
    /// (e.g. a tooling-only `pyproject.toml`). Other ecosystems publish further
    /// artifacts, so this is false for them.
    fn publishes_only_git_tag(&self) -> bool {
        false
    }
}

pub use detect::detect;
