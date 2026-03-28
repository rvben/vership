use vership::config::Config;

#[test]
fn parse_empty_config() {
    let config = Config::parse("").unwrap();
    assert_eq!(config.project.branch, "main");
    assert!(config.hooks.pre_bump.is_none());
}

#[test]
fn parse_full_config() {
    let toml = r#"
[project]
type = "rust-maturin"
branch = "develop"

[changelog]
unconventional = "include"

[hooks]
pre-bump = "make verify"
post-push = "echo done"

[checks]
lint = false
tests = false
"#;
    let config = Config::parse(toml).unwrap();
    assert_eq!(config.project.project_type.as_deref(), Some("rust-maturin"));
    assert_eq!(config.project.branch, "develop");
    assert_eq!(config.changelog.unconventional, "include");
    assert_eq!(config.hooks.pre_bump.as_deref(), Some("make verify"));
    assert_eq!(config.hooks.post_push.as_deref(), Some("echo done"));
    assert!(!config.checks.lint);
    assert!(!config.checks.tests);
}

#[test]
fn parse_partial_config_uses_defaults() {
    let toml = r#"
[hooks]
pre-bump = "make check"
"#;
    let config = Config::parse(toml).unwrap();
    assert_eq!(config.project.branch, "main");
    assert!(config.checks.lint);
    assert!(config.checks.tests);
}

#[test]
fn load_missing_file_returns_default() {
    let config = Config::load(std::path::Path::new("/nonexistent/vership.toml"));
    assert_eq!(config.project.branch, "main");
}

#[test]
fn parse_version_files_config() {
    let toml = r#"
[[version_files]]
glob = "README.md"
search = "rev: v{prev}"
replace = "rev: v{version}"

[[version_files]]
glob = "npm/*/package.json"
field = "version"
"#;
    let config = Config::parse(toml).unwrap();
    assert_eq!(config.version_files.len(), 2);

    let text_entry = &config.version_files[0];
    assert_eq!(text_entry.glob, "README.md");
    assert_eq!(text_entry.search.as_deref(), Some("rev: v{prev}"));
    assert_eq!(text_entry.replace.as_deref(), Some("rev: v{version}"));
    assert!(text_entry.field.is_none());

    let field_entry = &config.version_files[1];
    assert_eq!(field_entry.glob, "npm/*/package.json");
    assert!(field_entry.search.is_none());
    assert_eq!(field_entry.field.as_deref(), Some("version"));
}

#[test]
fn parse_artifacts_config() {
    let toml = r#"
[[artifacts]]
command = "cargo run -- rule -o json"
output = "rules.json"

[[artifacts]]
command = "cargo run -- schema generate"
files = ["schema.json"]
"#;
    let config = Config::parse(toml).unwrap();
    assert_eq!(config.artifacts.len(), 2);

    assert_eq!(config.artifacts[0].command, "cargo run -- rule -o json");
    assert_eq!(config.artifacts[0].output.as_deref(), Some("rules.json"));
    assert!(config.artifacts[0].files.is_empty());

    assert_eq!(config.artifacts[1].command, "cargo run -- schema generate");
    assert!(config.artifacts[1].output.is_none());
    assert_eq!(config.artifacts[1].files, vec!["schema.json"]);
}

#[test]
fn parse_empty_config_has_empty_version_files_and_artifacts() {
    let config = Config::parse("").unwrap();
    assert!(config.version_files.is_empty());
    assert!(config.artifacts.is_empty());
}
