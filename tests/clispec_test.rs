//! Integration tests asserting clispec v0.2 spec compliance.
//!
//! All tests exercise the real vership binary via assert_cmd so we verify the
//! production code path, not a re-implementation.

use std::fs;
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

// ---- helpers ----

/// Initialize a bare-minimum git repo so vership commands that need a repo work.
fn init_repo(dir: &std::path::Path) {
    for args in [
        vec!["init"],
        vec!["checkout", "-b", "main"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "tag.gpgsign", "false"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(dir)
            .output()
            .expect("git runs");
    }
    // An initial commit so HEAD exists.
    fs::write(dir.join("README.md"), "test").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "chore: init"])
        .current_dir(dir)
        .output()
        .unwrap();
    // Seed a Cargo.toml so project detection succeeds.
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
}

/// Extract the last non-empty line of stderr (the structured error envelope).
fn last_stderr_line(stderr: &[u8]) -> String {
    let s = String::from_utf8_lossy(stderr);
    s.lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string()
}

// ---- schema validation ----

/// The `schema` subcommand output must validate against the clispec v0.2 JSON Schema.
#[test]
fn schema_validates_against_clispec_v02() {
    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .clone();

    let schema_doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("schema is valid JSON");

    let meta_schema_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clispec-v0.2.json");
    let meta_schema_text = fs::read_to_string(&meta_schema_path)
        .unwrap_or_else(|_| panic!("fixture not found: {}", meta_schema_path.display()));
    let meta_schema: serde_json::Value =
        serde_json::from_str(&meta_schema_text).expect("meta-schema is valid JSON");

    let validator = jsonschema::validator_for(&meta_schema).expect("valid meta-schema");
    let errors: Vec<_> = validator.iter_errors(&schema_doc).collect();
    assert!(
        errors.is_empty(),
        "schema failed v0.2 validation:\n{}",
        errors
            .iter()
            .map(|e| format!("  - {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// All commands in the schema must carry a `mutating` boolean.
#[test]
fn schema_all_commands_have_mutating() {
    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let commands = doc["commands"].as_array().expect("commands is array");
    for cmd in commands {
        let name = cmd["name"].as_str().unwrap_or("?");
        assert!(
            cmd.get("mutating").and_then(|v| v.as_bool()).is_some(),
            "command '{name}' is missing mutating boolean"
        );
    }
}

/// `status` output fields must be declared in the schema.
#[test]
fn schema_status_has_output_fields() {
    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let commands = doc["commands"].as_array().expect("commands is array");
    let status = commands
        .iter()
        .find(|c| c["name"] == "status")
        .expect("status command in schema");
    let fields = status["output_fields"]
        .as_array()
        .expect("output_fields is array");
    assert!(
        !fields.is_empty(),
        "status command must have output_fields declared"
    );
}

/// The schema must declare global_args including --output.
#[test]
fn schema_has_global_args_with_output() {
    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let global_args = doc["global_args"].as_array().expect("global_args is array");
    assert!(
        !global_args.is_empty(),
        "global_args must be declared and non-empty"
    );
    let has_output = global_args
        .iter()
        .any(|a| a["name"] == "--output" || a["name"] == "-o");
    assert!(has_output, "--output must be in global_args");
}

// ---- auto-JSON when piped ----

/// When stdout is piped, `status` emits valid JSON without any explicit flag.
/// assert_cmd captures stdout via a pipe, so this exercises the auto-detect path.
#[test]
fn status_emits_json_when_piped() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["status"])
        .assert()
        .success()
        .get_output()
        .clone();

    // assert_cmd captures stdout via pipe, so auto-JSON should trigger.
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("status stdout must be valid JSON when piped");
    assert!(
        parsed.get("current_version").is_some(),
        "JSON must contain current_version"
    );
    assert!(
        parsed.get("project_type").is_some(),
        "JSON must contain project_type"
    );
}

// ---- explicit format wins ----

/// `-o text` piped must emit non-JSON text to stdout - explicit format beats TTY detection.
/// The scorer requires: stdout is non-empty AND not valid JSON.
#[test]
fn explicit_text_format_beats_pipe_detection() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["-o", "text", "status"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // Text mode must produce non-empty stdout.
    assert!(
        !stdout.trim().is_empty(),
        "text mode must produce output on stdout, got empty"
    );
    // It must not be JSON.
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "text mode stdout must not be JSON, got: {stdout}"
    );
    // Must contain human-readable data.
    assert!(
        stdout.contains("Current version"),
        "text mode must contain version info, got: {stdout}"
    );
}

/// `-o json` must emit valid JSON to stdout.
#[test]
fn explicit_json_format_emits_json() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["-o", "json", "status"])
        .assert()
        .success()
        .get_output()
        .clone();

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("-o json must emit valid JSON");
    assert!(parsed.get("current_version").is_some());
}

/// `--json` alias must still work (backward compatibility).
#[test]
fn json_flag_alias_still_works() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["--json", "status"])
        .assert()
        .success()
        .get_output()
        .clone();

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json must emit valid JSON");
    assert!(parsed.get("current_version").is_some());
}

// ---- structured error envelope ----

/// On error, the last line of stderr must be a JSON error envelope.
/// We trigger this by running `bump` without --yes and without a TTY.
#[test]
fn error_envelope_is_last_line_of_stderr() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["bump", "patch", "--skip-checks"])
        // assert_cmd pipes stdin so is_terminal() returns false.
        .assert()
        .failure()
        .get_output()
        .clone();

    let last = last_stderr_line(&output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(&last).expect("last stderr line must be valid JSON envelope");
    assert!(
        parsed.get("error").is_some(),
        "envelope must have 'error' key, got: {parsed}"
    );
    let err_obj = &parsed["error"];
    assert!(
        err_obj.get("kind").and_then(|v| v.as_str()).is_some(),
        "error must have 'kind' string"
    );
    assert!(
        err_obj.get("message").and_then(|v| v.as_str()).is_some(),
        "error must have 'message' string"
    );
}

// ---- conflict error kind ----

/// When the target tag already exists, vership bump must exit non-zero and emit
/// a structured envelope with `kind: "conflict"` and an exit code that matches
/// the schema declaration (7). Uses a Gradle project so the lockfile check is
/// trivially satisfied (no Cargo.lock needed).
///
/// Setup: repo at v0.1.0, tag v0.1.1 pre-created. `vership bump patch` would
/// target v0.1.1, hit the pre-flight tag-exists check, and must emit conflict.
#[test]
fn tag_already_exists_emits_conflict_kind() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    for args in [
        vec!["init"],
        vec!["checkout", "-b", "main"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "tag.gpgsign", "false"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .expect("git runs");
    }

    // Gradle project: settings.gradle.kts triggers detection, gradle.properties
    // holds the version. No Cargo.lock required.
    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        root.join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.0\n",
    )
    .unwrap();
    Command::new("git")
        .args(["add", "settings.gradle.kts", "gradle.properties"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "chore: init"])
        .current_dir(root)
        .output()
        .unwrap();
    // Establish v0.1.0 as the latest released tag.
    Command::new("git")
        .args(["tag", "v0.1.0"])
        .current_dir(root)
        .output()
        .unwrap();

    // A commit to give bump something to release.
    fs::write(root.join("change.txt"), "x").unwrap();
    Command::new("git")
        .args(["add", "change.txt"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "fix: something"])
        .current_dir(root)
        .output()
        .unwrap();

    // Pre-create v0.1.1 so the bump target collides.
    Command::new("git")
        .args(["tag", "v0.1.1"])
        .current_dir(root)
        .output()
        .unwrap();

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--skip-checks"])
        .assert()
        .failure()
        .get_output()
        .clone();

    // Exit code must match the schema declaration for conflict (7).
    let schema_output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .clone();
    let schema: serde_json::Value = serde_json::from_slice(&schema_output.stdout).unwrap();
    let conflict_entry = schema["errors"]
        .as_array()
        .expect("errors array in schema")
        .iter()
        .find(|e| e["kind"] == "conflict")
        .expect("conflict must be in schema errors");
    let schema_exit_code = conflict_entry["exit_code"]
        .as_i64()
        .expect("conflict exit_code must be a number") as i32;

    assert_eq!(
        output.status.code(),
        Some(schema_exit_code),
        "exit code must match schema declaration for conflict"
    );

    // The last stderr line must be a JSON envelope with kind "conflict".
    let last = last_stderr_line(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_str(&last).expect("last stderr line must be valid JSON envelope");
    assert_eq!(
        envelope["error"]["kind"].as_str(),
        Some("conflict"),
        "error kind must be 'conflict', got: {envelope}"
    );
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("v0.1.1"),
        "error message must mention the conflicting tag, got: {envelope}"
    );
}

// ---- dry-run behavior ----

/// `bump --dry-run` must succeed without a TTY (dry-run does not modify state).
///
/// Uses a Gradle project to avoid needing a valid Cargo.lock (Gradle lockfile
/// check is a no-op when gradle.properties exists).
#[test]
fn bump_dry_run_does_not_modify_state() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // Init repo.
    for args in [
        vec!["init"],
        vec!["checkout", "-b", "main"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "tag.gpgsign", "false"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .expect("git runs");
    }

    // Use a Gradle project so the lockfile check is trivially satisfied.
    fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        root.join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.0\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "settings.gradle.kts", "gradle.properties"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "chore: init"])
        .current_dir(root)
        .output()
        .unwrap();

    // A commit to release.
    fs::write(root.join("file.txt"), "change").unwrap();
    Command::new("git")
        .args(["add", "file.txt"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "fix: something"])
        .current_dir(root)
        .output()
        .unwrap();

    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .args(["bump", "patch", "--dry-run", "--skip-checks"])
        .assert()
        .success();
}

// ---- status --limit and --fields ----

/// `status` JSON output includes commits array.
#[test]
fn status_json_contains_commits() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["status"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        doc.get("commits").is_some(),
        "status JSON must have commits field"
    );
}

/// `status --fields current_version` only returns the requested field in JSON.
#[test]
fn status_fields_filter_json_output() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["status", "--fields", "current_version"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        doc.get("current_version").is_some(),
        "requested field must be present"
    );
    assert!(
        doc.get("project_type").is_none(),
        "non-selected field must be absent"
    );
}

/// `status --limit 1` with 2+ commits must set truncated=true in JSON.
#[test]
fn status_limit_sets_truncated_when_exceeded() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    // Add two commits after the initial one.
    for i in 0..2 {
        fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
        Command::new("git")
            .args(["add", &format!("f{i}.txt")])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", &format!("fix: change {i}")])
            .current_dir(dir.path())
            .output()
            .unwrap();
    }

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["status", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        doc["truncated"].as_bool(),
        Some(true),
        "truncated must be true when limit is exceeded"
    );
    assert!(
        doc["total_commits"].as_u64().unwrap_or(0) >= 2,
        "total_commits must reflect actual count"
    );
    let commits = doc["commits"].as_array().expect("commits array");
    assert_eq!(commits.len(), 1, "only 1 commit returned when limit=1");
}

#[test]
fn help_and_version_write_no_error_envelope() {
    for flag in ["--help", "--version"] {
        let output = AssertCommand::cargo_bin("vership")
            .unwrap()
            .arg(flag)
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(
            output.stderr.is_empty(),
            "{flag} must write nothing to stderr, got: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn status_fields_selects_computed_commits() {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(dir.path())
        .args(["status", "--fields", "commits", "-o", "json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let keys: Vec<&String> = doc.as_object().expect("object").keys().collect();
    assert_eq!(
        keys,
        vec!["commits"],
        "--fields commits must select exactly the computed commits field"
    );
    assert!(doc["commits"].is_array(), "commits must be an array");
}
