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
