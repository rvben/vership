use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub project: ProjectConfig,
    pub changelog: ChangelogConfig,
    pub hooks: HooksConfig,
    pub checks: ChecksConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    #[serde(rename = "type")]
    pub project_type: Option<String>,
    pub branch: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ChangelogConfig {
    pub unconventional: String,
    pub exclude_types: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    #[serde(rename = "pre-bump")]
    pub pre_bump: Option<String>,
    #[serde(rename = "post-bump")]
    pub post_bump: Option<String>,
    #[serde(rename = "pre-push")]
    pub pre_push: Option<String>,
    #[serde(rename = "post-push")]
    pub post_push: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ChecksConfig {
    pub lint: bool,
    pub tests: bool,
    pub lint_command: Option<String>,
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

pub fn init() -> Result<()> {
    let path = Path::new("vership.toml");
    if path.exists() {
        eprintln!("vership.toml already exists");
        return Ok(());
    }

    let template = r#"# vership.toml — optional configuration for vership
# All settings below show their defaults. Uncomment to override.

# [project]
# type = "rust"        # Override auto-detection: "rust", "rust-maturin"
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
"#;

    std::fs::write(path, template)?;
    eprintln!("Created vership.toml");
    Ok(())
}
