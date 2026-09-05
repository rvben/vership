use assert_cmd::Command as AssertCommand;
use vership::changelog::CuratedPolicy;
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
allow_untracked = true
"#;
    let config = Config::parse(toml).unwrap();
    assert_eq!(config.project.project_type.as_deref(), Some("rust-maturin"));
    assert_eq!(config.project.branch, "develop");
    assert_eq!(config.changelog.unconventional, "include");
    assert_eq!(config.hooks.pre_bump.as_deref(), Some("make verify"));
    assert_eq!(config.hooks.post_push.as_deref(), Some("echo done"));
    assert!(!config.checks.lint);
    assert!(!config.checks.tests);
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("vership.toml");
    std::fs::write(&path, toml).unwrap();
    assert!(Config::load_allow_untracked(&path).unwrap());
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
    let dir = tempfile::TempDir::new().unwrap();
    assert!(!Config::load_allow_untracked(&dir.path().join("missing.toml")).unwrap());
}

#[test]
fn invalid_unconventional_mode_fails_closed() {
    let error = Config::parse("[changelog]\nunconventional = \"strcit\"\n")
        .expect_err("a policy typo must not silently weaken strict mode");
    assert!(error.to_string().contains("exclude, include, or strict"));
}

#[test]
fn curated_policy_defaults_to_merge_and_reads_replace() {
    let dir = tempfile::TempDir::new().unwrap();
    assert_eq!(
        Config::load_curated_policy(&dir.path().join("missing.toml")).unwrap(),
        CuratedPolicy::Merge
    );

    let path = dir.path().join("vership.toml");
    std::fs::write(&path, "[changelog]\nunconventional = \"include\"\n").unwrap();
    assert_eq!(
        Config::load_curated_policy(&path).unwrap(),
        CuratedPolicy::Merge,
        "a changelog section without the key keeps the default"
    );

    std::fs::write(&path, "[changelog]\ncurated = \"replace\"\n").unwrap();
    assert_eq!(
        Config::load_curated_policy(&path).unwrap(),
        CuratedPolicy::Replace
    );
}

#[test]
fn invalid_curated_policy_fails_closed() {
    let error = Config::parse("[changelog]\ncurated = \"Merge\"\n")
        .expect_err("an unknown curated policy must not silently fall back to a default");
    assert!(
        error
            .to_string()
            .contains("changelog.curated must be merge or replace; got \"Merge\""),
        "got: {error}"
    );

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("vership.toml");
    std::fs::write(&path, "[changelog]\ncurated = \"keep\"\n").unwrap();
    let error = Config::load_curated_policy(&path).expect_err("the loader rejects it too");
    assert!(error.to_string().contains("must be merge or replace"));
}

#[test]
fn load_missing_file_returns_default() {
    let config = Config::load_checked(std::path::Path::new("/nonexistent/vership.toml")).unwrap();
    assert_eq!(config.project.branch, "main");
}

#[test]
fn load_malformed_file_returns_an_error_instead_of_defaults() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("vership.toml");
    std::fs::write(&path, "[checks\ntests = false\n").unwrap();

    let error =
        Config::load_checked(&path).expect_err("invalid release configuration must fail closed");
    assert!(error.to_string().contains("parse vership.toml"));
}

#[test]
fn config_show_includes_the_effective_untracked_policy_without_a_config_file() {
    let dir = tempfile::TempDir::new().unwrap();

    let text = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "show", "--output", "text"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains("allow_untracked = false"), "got:\n{text}");
    let reparsed: toml::Value = toml::from_str(&text).expect("text output must be valid TOML");
    assert_eq!(reparsed["checks"]["allow_untracked"].as_bool(), Some(false));
    assert_eq!(reparsed["changelog"]["curated"].as_str(), Some("merge"));

    let json = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "show", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["checks"]["allow_untracked"], false);
    assert_eq!(json["changelog"]["curated"], "merge");
}

#[test]
fn config_show_reports_a_configured_curated_policy() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("vership.toml"),
        "[changelog]\ncurated = \"replace\"\n",
    )
    .unwrap();

    let json = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "show", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["changelog"]["curated"], "replace");
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

#[test]
fn verify_section_parses() {
    let config = Config::parse(
        r#"
[verify]
skip = ["npm"]
tap = "rvben/homebrew-tap"
formula = "vership"
image = "rvben/vership"
"#,
    )
    .unwrap();
    assert_eq!(config.verify.skip, vec!["npm"]);
    assert_eq!(config.verify.tap.as_deref(), Some("rvben/homebrew-tap"));
    assert_eq!(config.verify.formula.as_deref(), Some("vership"));
    assert_eq!(config.verify.image.as_deref(), Some("rvben/vership"));
}

#[test]
fn verify_section_defaults_to_empty() {
    let config = Config::parse("").unwrap();
    assert!(config.verify.skip.is_empty());
    assert!(config.verify.tap.is_none());
}
