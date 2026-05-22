use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::ProjectType;
use crate::error::{Error, Result};
use crate::version;

/// Where a Gradle project keeps its version.
enum Source {
    /// A key in `gradle.properties` (e.g. `pluginVersion` or `version`).
    Property { key: &'static str },
    /// A `version = "x.y.z"` assignment in a build script.
    BuildScript { file: &'static str },
}

impl Source {
    /// File name (relative to the project root) that holds the version.
    fn file(&self) -> &'static str {
        match self {
            Source::Property { .. } => "gradle.properties",
            Source::BuildScript { file } => file,
        }
    }
}

/// Gradle / JetBrains plugin projects.
///
/// The version is read from, in priority order:
///   1. `gradle.properties` `pluginVersion` (IntelliJ Platform plugin convention)
///   2. `gradle.properties` `version`
///   3. `build.gradle.kts` / `build.gradle` `version = "..."`
pub struct GradleProject {
    modified_files: RefCell<Vec<PathBuf>>,
}

impl GradleProject {
    pub fn new() -> Self {
        Self {
            modified_files: RefCell::new(Vec::new()),
        }
    }

    /// Locate the file and key that own the project version.
    fn resolve_source(root: &Path) -> Result<Source> {
        let props = root.join("gradle.properties");
        if props.exists() {
            let content = std::fs::read_to_string(&props)
                .map_err(|e| Error::Other(format!("read gradle.properties: {e}")))?;
            for key in ["pluginVersion", "version"] {
                if version::parse_gradle_property(&content, key).is_some() {
                    return Ok(Source::Property { key });
                }
            }
        }

        for file in ["build.gradle.kts", "build.gradle"] {
            let path = root.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| Error::Other(format!("read {file}: {e}")))?;
                if version::parse_gradle_buildscript_version(&content).is_some() {
                    return Ok(Source::BuildScript { file });
                }
            }
        }

        Err(Error::Version(
            "no version found: expected `pluginVersion`/`version` in gradle.properties \
             or `version = \"...\"` in build.gradle[.kts]"
                .to_string(),
        ))
    }
}

impl Default for GradleProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectType for GradleProject {
    fn name(&self) -> &str {
        "Gradle"
    }

    fn read_version(&self, root: &Path) -> Result<semver::Version> {
        let source = Self::resolve_source(root)?;
        let path = root.join(source.file());
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read {}: {e}", source.file())))?;

        let raw = match &source {
            Source::Property { key } => version::parse_gradle_property(&content, key),
            Source::BuildScript { .. } => version::parse_gradle_buildscript_version(&content),
        }
        .ok_or_else(|| Error::Version(format!("no version in {}", source.file())))?;

        semver::Version::parse(&raw)
            .map_err(|e| Error::Version(format!("invalid version '{raw}': {e}")))
    }

    fn write_version(&self, root: &Path, new_version: &semver::Version) -> Result<()> {
        let mut modified = self.modified_files.borrow_mut();
        modified.clear();

        let source = Self::resolve_source(root)?;
        let file = source.file();
        let path = root.join(file);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read {file}: {e}")))?;

        let updated = match &source {
            Source::Property { key } => {
                version::replace_gradle_property(&content, key, new_version)
            }
            Source::BuildScript { .. } => {
                version::replace_gradle_buildscript_version(&content, new_version)
            }
        }
        .ok_or_else(|| Error::Version(format!("cannot update version in {file}")))?;

        std::fs::write(&path, updated).map_err(|e| Error::Other(format!("write {file}: {e}")))?;
        modified.push(PathBuf::from(file));

        Ok(())
    }

    fn verify_lockfile(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn sync_lockfile(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn run_lint(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn run_tests(&self, _root: &Path) -> Result<()> {
        Ok(())
    }

    fn modified_files(&self) -> Vec<PathBuf> {
        self.modified_files.borrow().clone()
    }
}
