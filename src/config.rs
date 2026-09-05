use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::changelog::CuratedPolicy;
use crate::error::{Error, Result};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub project: ProjectConfig,
    pub changelog: ChangelogConfig,
    pub hooks: HooksConfig,
    pub checks: ChecksConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_files: Vec<VersionFileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactEntry>,
    pub verify: VerifyConfig,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectConfig {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub project_type: Option<String>,
    pub branch: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ChangelogConfig {
    pub unconventional: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_types: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct HooksConfig {
    #[serde(rename = "pre-bump", skip_serializing_if = "Option::is_none")]
    pub pre_bump: Option<String>,
    #[serde(rename = "post-bump", skip_serializing_if = "Option::is_none")]
    pub post_bump: Option<String>,
    #[serde(rename = "pre-push", skip_serializing_if = "Option::is_none")]
    pub pre_push: Option<String>,
    #[serde(rename = "post-push", skip_serializing_if = "Option::is_none")]
    pub post_push: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct VersionFileEntry {
    pub glob: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ArtifactEntry {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ChecksConfig {
    pub lint: bool,
    pub tests: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_command: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct VerifyConfig {
    /// Target names to skip (tag, release, crates, pypi, npm, homebrew, ghcr).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<String>,
    /// Homebrew tap as owner/repo. Default: <owner>/homebrew-tap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    /// Formula name in the tap. Default: the repository name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// ghcr image as owner/name. Default: <owner>/<repo> lowercased.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project_type: None,
            branch: "main".to_string(),
        }
    }
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            unconventional: "exclude".to_string(),
            exclude_types: Vec::new(),
        }
    }
}

impl Default for ChecksConfig {
    fn default() -> Self {
        Self {
            lint: true,
            tests: true,
            lint_command: None,
            test_command: None,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SafetyConfig {
    checks: SafetyChecks,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SafetyChecks {
    allow_untracked: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CuratedConfig {
    changelog: CuratedChangelog,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CuratedChangelog {
    curated: Option<String>,
}

fn parse_curated_policy(content: &str) -> Result<CuratedPolicy> {
    let config: CuratedConfig =
        toml::from_str(content).map_err(|e| Error::Config(format!("parse vership.toml: {e}")))?;
    match config.changelog.curated.as_deref() {
        None => Ok(CuratedPolicy::default()),
        Some(value) => CuratedPolicy::parse(value).ok_or_else(|| {
            Error::Config(format!(
                "changelog.curated must be merge or replace; got {value:?}"
            ))
        }),
    }
}

impl Config {
    pub fn parse(content: &str) -> Result<Self> {
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        let config: Self = toml::from_str(content)
            .map_err(|e| Error::Config(format!("parse vership.toml: {e}")))?;
        let _: SafetyConfig = toml::from_str(content)
            .map_err(|e| Error::Config(format!("parse vership.toml: {e}")))?;
        parse_curated_policy(content)?;
        match config.changelog.unconventional.as_str() {
            "exclude" | "include" | "strict" => Ok(config),
            value => Err(Error::Config(format!(
                "changelog.unconventional must be exclude, include, or strict; got {value:?}"
            ))),
        }
    }

    /// Read how curated `## [Unreleased]` notes combine with generated entries
    /// (`changelog.curated`), without expanding the stable public
    /// `ChangelogConfig` shape. A missing file or key means merge.
    pub fn load_curated_policy(path: &Path) -> Result<CuratedPolicy> {
        match std::fs::read_to_string(path) {
            Ok(content) => parse_curated_policy(&content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(CuratedPolicy::default())
            }
            Err(error) => Err(Error::Io(error)),
        }
    }

    /// Read the release-safety opt-out without expanding the stable public
    /// `ChecksConfig` shape in a patch release.
    pub fn load_allow_untracked(path: &Path) -> Result<bool> {
        match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str::<SafetyConfig>(&content)
                .map(|config| config.checks.allow_untracked)
                .map_err(|e| Error::Config(format!("parse vership.toml: {e}"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::Io(error)),
        }
    }

    /// Load configuration and report malformed or unreadable files.
    pub fn load_checked(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::parse(&content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(Error::Io(error)),
        }
    }

    /// Compatibility wrapper for library users. CLI paths use `load_checked`
    /// and fail closed; new callers should do the same.
    /// Legacy fail-open loader retained for patch-line compatibility. New CLI
    /// code uses [`Config::load_checked`] so malformed release policy fails.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| Self::parse(&content).ok())
            .unwrap_or_default()
    }
}

pub fn show(output: &crate::output::OutputConfig) -> Result<()> {
    let path = Path::new("vership.toml");
    let config = Config::load_checked(path)?;
    let allow_untracked = Config::load_allow_untracked(path)?;
    let curated = Config::load_curated_policy(path)?;
    if output.is_json() {
        let mut value = serde_json::to_value(&config).map_err(|e| Error::Config(e.to_string()))?;
        let root = value
            .as_object_mut()
            .ok_or_else(|| Error::Config("serialized config is not an object".to_string()))?;
        root.entry("checks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| Error::Config("serialized checks config is not an object".to_string()))?
            .insert(
                "allow_untracked".to_string(),
                serde_json::json!(allow_untracked),
            );
        root.entry("changelog")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                Error::Config("serialized changelog config is not an object".to_string())
            })?
            .insert("curated".to_string(), serde_json::json!(curated.as_str()));
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| Error::Config(e.to_string()))?
        );
    } else {
        let mut value = toml::Value::try_from(&config).map_err(|e| Error::Config(e.to_string()))?;
        let root = value
            .as_table_mut()
            .ok_or_else(|| Error::Config("serialized config is not a table".to_string()))?;
        root.entry("checks")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| Error::Config("serialized checks config is not a table".to_string()))?
            .insert(
                "allow_untracked".to_string(),
                toml::Value::Boolean(allow_untracked),
            );
        root.entry("changelog")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| Error::Config("serialized changelog config is not a table".to_string()))?
            .insert(
                "curated".to_string(),
                toml::Value::String(curated.as_str().to_string()),
            );
        print!(
            "{}",
            toml::to_string_pretty(&value).map_err(|e| Error::Config(e.to_string()))?
        );
    }
    Ok(())
}

pub fn init() -> Result<()> {
    let path = Path::new("vership.toml");
    if path.exists() {
        eprintln!("vership.toml already exists");
        return Ok(());
    }

    let template = r#"# vership.toml — optional configuration for vership
# All settings below show their defaults. Uncomment to override.

# [project]
# type = "rust"        # Override auto-detection: "rust", "rust-maturin", "node", "go", "python", "gradle", "ansible-collection"
# branch = "main"      # Branch to release from

# [changelog]
# unconventional = "exclude"   # "exclude", "include", or "strict"
# exclude_types = []           # Additional commit types to exclude
# curated = "merge"            # Curated [Unreleased] notes: "merge" adds the generated
#                              # entries they do not cite; "replace" drops every generated entry

# [hooks]
# pre-bump = ""
# post-bump = ""
# pre-push = ""
# post-push = ""

# [checks]
# lint = true
# tests = true
# allow_untracked = false          # Opt out of strict clean-tree checks
# lint_command = "npm run lint"    # Override default lint command
# test_command = "npm test"       # Override default test command

# Update version references in extra files during bump
# [[version_files]]
# glob = "README.md"
# search = "v{prev}"             # Text mode: {prev} = old version
# replace = "v{version}"         # Text mode: {version} = new version
#
# [[version_files]]
# glob = "npm/*/package.json"
# field = "version"              # Field mode: update JSON field directly
#
# [[version_files]]
# glob = "package.json"
# field = "optionalDependencies.*"  # Wildcard: update all values in object

# Regenerate files from commands during bump
# [[artifacts]]
# command = "cargo run -- schema generate"
# output = "schema.json"         # Capture stdout to file
#
# [[artifacts]]
# command = "make generate"
# files = ["generated.json"]     # Files the command produces

# Post-release verification targets (vership verify)
# [verify]
# skip = []                        # Targets to skip: tag, release, crates, pypi, npm, homebrew, ghcr
# tap = "owner/homebrew-tap"       # Homebrew tap repo
# formula = "name"                 # Formula name (default: repo name)
# image = "owner/name"             # ghcr image (default: owner/repo lowercased)
"#;

    std::fs::write(path, template)?;
    eprintln!("Created vership.toml");
    Ok(())
}
