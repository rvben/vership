pub mod detect;
pub mod go;
pub mod node;
pub mod python;
pub mod rust;
pub mod rust_maturin;

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

    /// Whether the version source is the git tag rather than a project file.
    /// When true, release uses "chore: release" instead of "chore: bump version to".
    fn is_tag_versioned(&self) -> bool {
        false
    }
}

pub use detect::detect;
