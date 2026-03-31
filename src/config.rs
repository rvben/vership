use std::path::Path;

use serde::{Deserialize, Serialize};

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

impl Config {
    pub fn parse(content: &str) -> Result<Self> {
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        toml::from_str(content).map_err(|e| Error::Config(format!("parse vership.toml: {e}")))
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::parse(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

pub fn show(json: bool) -> Result<()> {
    let path = Path::new("vership.toml");
    let config = Config::load(path);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config).map_err(|e| Error::Config(e.to_string()))?
        );
    } else {
        let toml = toml::to_string_pretty(&config).map_err(|e| Error::Config(e.to_string()))?;
        print!("{toml}");
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
# type = "rust"        # Override auto-detection: "rust", "rust-maturin", "node", "go", "python"
# branch = "main"      # Branch to release from

# [changelog]
# unconventional = "exclude"   # "exclude", "include", or "strict"
# exclude_types = []           # Additional commit types to exclude

# [hooks]
# pre-bump = ""
# post-bump = ""
# pre-push = ""
# post-push = ""

# [checks]
# lint = true
# tests = true
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
"#;

    std::fs::write(path, template)?;
    eprintln!("Created vership.toml");
    Ok(())
}
