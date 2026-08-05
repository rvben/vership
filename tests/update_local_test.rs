//! End-to-end tests for `vership update-local`.
//!
//! Every test drives the real binary, and every install the binary finds is a
//! real `cargo install --list` answer: the tests point `CARGO_HOME` at a
//! fixture directory holding a hand-written `.crates.toml`, so cargo itself
//! reports the install. Nothing is stubbed and nothing on the developer's
//! machine is touched.
//!
//! No test reaches the network. The only install fixture is a `--path` install,
//! which no registry speaks for, so the pre-install registry gate is skipped by
//! design rather than by mocking.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

/// A project directory whose Cargo.toml names `crate_name` at `version`.
fn project(crate_name: &str, version: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"{version}\"\nedition = \"2021\"\n"
        ),
    )
    .unwrap();
    dir
}

/// A virtual workspace: a root declaring no package of its own, and one member
/// crate at `crates/<crate_name>` with a binary. Returns the root, whose member
/// directory is what `cargo install --path` would record.
fn workspace(crate_name: &str, version: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"{version}\"\n"
        ),
    )
    .unwrap();
    let member = dir.path().join("crates").join(crate_name);
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(
        member.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{crate_name}\"\nversion.workspace = true\nedition = \"2021\"\n"
        ),
    )
    .unwrap();
    fs::write(member.join("src/main.rs"), "fn main() {}\n").unwrap();
    dir
}

/// The directory of the member crate `workspace` created.
fn member_dir(root: &Path, crate_name: &str) -> PathBuf {
    root.join("crates").join(crate_name)
}

/// A `CARGO_HOME` recording the given `cargo install` entries, in the format
/// cargo reads them back from.
fn cargo_home(entries: &[String]) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("bin")).unwrap();
    fs::write(
        dir.path().join(".crates.toml"),
        format!("[v1]\n{}\n", entries.join("\n")),
    )
    .unwrap();
    dir
}

/// A `.crates.toml` entry for a `cargo install --path` install.
fn path_entry(crate_name: &str, version: &str, path: &Path, bin: &str) -> String {
    format!(
        "\"{crate_name} {version} (path+file://{})\" = [\"{bin}\"]",
        path.display()
    )
}

fn update_local(root: &Path, cargo_home: &Path, args: &[&str]) -> std::process::Output {
    AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root)
        .env("CARGO_HOME", cargo_home)
        // Cargo resolves the install root from CARGO_INSTALL_ROOT first, so a
        // value inherited from the surrounding test run would point the probe
        // somewhere other than the fixture.
        .env_remove("CARGO_INSTALL_ROOT")
        .arg("update-local")
        .args(args)
        .output()
        .expect("vership runs")
}

/// A `$PATH` whose first entries are `first`, followed by the inherited one so
/// the probe can still find `cargo` itself.
fn path_with(first: &[&Path]) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = first.iter().map(|p| p.to_path_buf()).collect();
    if let Some(inherited) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(dirs).unwrap()
}

/// Put an executable of `name` in `dir`.
fn write_executable(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name);
    fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    file
}

fn json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn a_package_installed_nowhere_locally_is_a_clean_pass() {
    let root = project("vership-e2e-absent", "0.2.0");
    let home = cargo_home(&[]);

    let output = update_local(root.path(), home.path(), &["-o", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let doc = json(&output);
    assert_eq!(doc["version"], "0.2.0");
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["changed"], false);
    assert_eq!(doc["installs"].as_array().unwrap().len(), 0);
    assert_eq!(doc["binaries"].as_array().unwrap().len(), 0);
}

#[test]
fn a_stale_copy_shadowing_a_workspace_binary_is_not_a_clean_pass() {
    let root = workspace("vership-e2e-demo", "0.2.0");
    // Nothing installed by any manager, which is what a workspace used to
    // produce whether or not anything was installed: the root declares no
    // package, so no package name was ever probed for.
    let home = cargo_home(&[]);
    let unmanaged = TempDir::new().unwrap();
    write_executable(unmanaged.path(), "vership-e2e-demo");

    let run = |dirs: &[&Path]| {
        AssertCommand::cargo_bin("vership")
            .unwrap()
            .current_dir(root.path())
            .env("CARGO_HOME", home.path())
            .env_remove("CARGO_INSTALL_ROOT")
            .env("PATH", path_with(dirs))
            .args(["update-local", "--managers", "cargo", "-o", "json"])
            .output()
            .expect("vership runs")
    };

    let output = run(&[unmanaged.path()]);
    let doc = json(&output);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a stale copy on PATH must not pass: {doc}"
    );
    let binary = &doc["binaries"].as_array().unwrap()[0];
    assert_eq!(binary["name"], "vership-e2e-demo");
    assert_eq!(
        fs::canonicalize(binary["path"].as_str().unwrap()).unwrap(),
        fs::canonicalize(unmanaged.path().join("vership-e2e-demo")).unwrap(),
        "the copy that wins PATH must be the one reported"
    );

    // The control: the same project with that copy off PATH passes, so the
    // failure above is the stale binary and not the workspace itself.
    let empty = TempDir::new().unwrap();
    let output = run(&[empty.path()]);
    let doc = json(&output);
    assert_eq!(output.status.code(), Some(0), "{doc}");
    assert_eq!(
        doc["binaries"].as_array().unwrap()[0]["path"],
        serde_json::Value::Null,
        "the name is still scanned for, and simply not found"
    );
}

#[test]
fn a_workspace_members_install_belongs_to_this_project() {
    let root = workspace("vership-e2e-demo", "0.2.0");
    // `cargo install --path .` run at the workspace root records the member
    // directory, not the root. Judging ownership by the root alone reads this
    // as another project's install and leaves it behind.
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.1.0",
        &member_dir(root.path(), "vership-e2e-demo"),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--dry-run", "--managers", "cargo", "-o", "json"],
    );

    let doc = json(&output);
    assert_eq!(output.status.code(), Some(0), "{doc}");
    let installs = doc["installs"].as_array().unwrap();
    assert_eq!(
        installs.len(),
        1,
        "the member's install must be found: {doc}"
    );
    assert_eq!(installs[0]["action"], "planned");
    let argv: Vec<&str> = installs[0]["commands"].as_array().unwrap()[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        fs::canonicalize(argv[3]).unwrap(),
        fs::canonicalize(member_dir(root.path(), "vership-e2e-demo")).unwrap(),
        "the rebuild must point at the member crate: {argv:?}"
    );
}

#[test]
fn nothing_installed_reports_what_was_looked_for() {
    let root = project("vership-e2e-absent", "0.2.0");
    let home = cargo_home(&[]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--managers", "cargo", "-o", "json"],
    );

    let doc = json(&output);
    assert_eq!(output.status.code(), Some(0), "{doc}");
    assert_eq!(doc["installs"].as_array().unwrap().len(), 0);
    // An empty `installs` alone cannot say whether this project has nothing a
    // manager could hold or has something that is simply not installed.
    assert_eq!(
        doc["considered"],
        serde_json::json!([{"manager": "cargo", "packages": ["vership-e2e-absent"]}])
    );

    // The control: a project with no manifest any manager reads considers
    // nothing, and says so by naming nothing rather than by the same empty list.
    let bare = TempDir::new().unwrap();
    let output = update_local(
        bare.path(),
        home.path(),
        &["0.2.0", "--managers", "cargo", "-o", "json"],
    );
    let doc = json(&output);
    assert_eq!(output.status.code(), Some(0), "{doc}");
    assert_eq!(doc["considered"], serde_json::json!([]));
}

#[test]
fn dry_run_reports_the_install_command_without_running_it() {
    let root = project("vership-e2e-demo", "0.2.0");
    // The install cargo will report is one patch behind, and built from this
    // very directory, so refreshing it is both needed and possible.
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.1.0",
        root.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--dry-run", "--managers", "cargo", "-o", "json"],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json(&output);
    assert_eq!(doc["dry_run"], true);
    assert_eq!(doc["changed"], false);

    let installs = doc["installs"].as_array().unwrap();
    assert_eq!(installs.len(), 1, "the fixture install must be found");
    let install = &installs[0];
    assert_eq!(install["manager"], "cargo");
    assert_eq!(install["package"], "vership-e2e-demo");
    assert_eq!(install["before"], "0.1.0");
    assert_eq!(install["after"], serde_json::Value::Null);
    assert_eq!(install["action"], "planned");

    let commands = install["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 1);
    let argv: Vec<&str> = commands[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(argv[0..3], ["cargo", "install", "--path"]);
    assert_eq!(argv[4], "--force");
    assert_eq!(
        fs::canonicalize(argv[3]).unwrap(),
        fs::canonicalize(root.path()).unwrap(),
        "the planned rebuild must point at this project"
    );

    // Nothing ran: cargo still records the old version and installed no
    // executable.
    let recorded = fs::read_to_string(home.path().join(".crates.toml")).unwrap();
    assert!(recorded.contains("0.1.0"), "{recorded}");
    assert_eq!(
        fs::read_dir(home.path().join("bin")).unwrap().count(),
        0,
        "dry run must not install an executable"
    );
}

#[test]
fn dry_run_prints_the_install_command_in_text_mode() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.1.0",
        root.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--dry-run", "--managers", "cargo", "-o", "text"],
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("update-local 0.2.0 (dry run)"),
        "text output must say what it would do: {stdout}"
    );
    assert!(
        stdout.contains("cargo install --path"),
        "text output must show the command: {stdout}"
    );
    assert!(
        stdout.contains("plan cargo"),
        "the row must be marked as planned: {stdout}"
    );
}

#[test]
fn an_install_of_another_project_is_left_alone() {
    let root = project("vership-e2e-demo", "0.2.0");
    let elsewhere = TempDir::new().unwrap();
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.1.0",
        elsewhere.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--dry-run", "--managers", "cargo", "-o", "json"],
    );

    assert_eq!(output.status.code(), Some(0));
    let doc = json(&output);
    let install = &doc["installs"].as_array().unwrap()[0];
    assert_eq!(install["action"], "skipped");
    assert_eq!(install["commands"].as_array().unwrap().len(), 0);
    assert!(
        install["detail"]
            .as_str()
            .unwrap_or("")
            .contains("not this project"),
        "the reason must name the other project: {install}"
    );
}

#[test]
fn an_install_already_at_the_target_version_needs_no_command() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.2.0",
        root.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--managers", "cargo", "-o", "json"],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json(&output);
    let install = &doc["installs"].as_array().unwrap()[0];
    assert_eq!(install["action"], "already-current");
    assert_eq!(install["after"], "0.2.0");
    assert_eq!(install["commands"].as_array().unwrap().len(), 0);
    assert_eq!(doc["changed"], false);
    assert_eq!(doc["ok"], true);
}

#[test]
fn an_explicit_version_overrides_the_on_disk_one() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.3.0",
        root.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["v0.3.0", "--managers", "cargo", "-o", "json"],
    );

    assert_eq!(output.status.code(), Some(0));
    let doc = json(&output);
    assert_eq!(doc["version"], "0.3.0", "the leading v must be stripped");
    assert_eq!(
        doc["installs"].as_array().unwrap()[0]["action"],
        "already-current",
        "the explicit version, not the Cargo.toml version, decides"
    );
}

#[test]
fn an_unknown_manager_is_a_config_error() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[]);

    let output = update_local(root.path(), home.path(), &["--managers", "pipx"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
    let envelope: serde_json::Value =
        serde_json::from_str(last).expect("last stderr line must be the error envelope");
    assert_eq!(envelope["error"]["kind"], "config");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("pipx"),
        "{envelope}"
    );
}

#[test]
fn the_binary_on_path_is_reported_with_the_copies_behind_it() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.2.0",
        root.path(),
        "vership-e2e-demo",
    )]);
    // Two directories on PATH holding the same executable name: the cargo one
    // the fixture install owns, and an unmanaged copy behind it.
    let bin_dir: PathBuf = home.path().join("bin");
    let unmanaged = TempDir::new().unwrap();
    let winner = write_executable(&bin_dir, "vership-e2e-demo");
    write_executable(unmanaged.path(), "vership-e2e-demo");

    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root.path())
        .env("CARGO_HOME", home.path())
        .env_remove("CARGO_INSTALL_ROOT")
        .env("PATH", path_with(&[&bin_dir, unmanaged.path()]))
        .args(["update-local", "--managers", "cargo", "-o", "json"])
        .output()
        .expect("vership runs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json(&output);
    let binaries = doc["binaries"].as_array().unwrap();
    assert_eq!(binaries.len(), 1);
    let binary = &binaries[0];
    assert_eq!(binary["name"], "vership-e2e-demo");
    assert_eq!(binary["manager"], "cargo");
    assert_eq!(binary["version"], "0.2.0");
    assert_eq!(
        fs::canonicalize(binary["path"].as_str().unwrap()).unwrap(),
        fs::canonicalize(&winner).unwrap()
    );
    let shadowed = binary["shadowed"].as_array().unwrap();
    assert_eq!(shadowed.len(), 1, "the second copy must be reported");
    assert_eq!(
        shadowed[0]["manager"],
        serde_json::Value::Null,
        "a copy no manager owns is reported as unmanaged, not attributed"
    );
    assert_eq!(shadowed[0]["version"], serde_json::Value::Null);
}

#[test]
fn a_manifest_no_selected_manager_needs_is_never_read() {
    let root = project("vership-e2e-demo", "0.2.0");
    fs::write(root.path().join("package.json"), "{ not json at all").unwrap();
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.2.0",
        root.path(),
        "vership-e2e-demo",
    )]);

    let output = update_local(
        root.path(),
        home.path(),
        &["--managers", "cargo", "-o", "json"],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "a cargo-only run must not read package.json: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json(&output);
    assert_eq!(doc["installs"].as_array().unwrap()[0]["manager"], "cargo");

    // The control: the same file does break a run that selects npm, so the
    // pass above is the filtering working, not an unparseable file parsing.
    let output = update_local(root.path(), home.path(), &["--managers", "npm"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse package.json"),
        "the control must fail on the manifest itself: {stderr}"
    );
}

#[test]
fn a_cargo_manifest_is_not_read_by_a_run_that_excludes_cargo() {
    // The mirror of the test above, and the one a workspace makes easy to
    // break: Cargo.toml is now read for the member directories and binary
    // names, which every manager's run needs eventually and only a cargo run
    // needs at all.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("Cargo.toml"), "[package\nname = broken\n").unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"name": "vership-e2e-demo", "version": "0.2.0"}"#,
    )
    .unwrap();
    let home = cargo_home(&[]);

    let output = update_local(
        root.path(),
        home.path(),
        &["0.2.0", "--managers", "npm", "-o", "json"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "an npm-only run must not read Cargo.toml: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The control: the same file does break a run that selects cargo.
    let output = update_local(root.path(), home.path(), &["0.2.0", "--managers", "cargo"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse ./Cargo.toml"),
        "the control must fail on the manifest itself: {stderr}"
    );
}

#[test]
fn an_unmanaged_copy_ahead_of_the_updated_one_fails() {
    let root = project("vership-e2e-demo", "0.2.0");
    let home = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.2.0",
        root.path(),
        "vership-e2e-demo",
    )]);
    let bin_dir = home.path().join("bin");
    let unmanaged = TempDir::new().unwrap();
    write_executable(&bin_dir, "vership-e2e-demo");
    write_executable(unmanaged.path(), "vership-e2e-demo");

    // The same two copies as the passing case, in the other order.
    let output = AssertCommand::cargo_bin("vership")
        .unwrap()
        .current_dir(root.path())
        .env("CARGO_HOME", home.path())
        .env_remove("CARGO_INSTALL_ROOT")
        .env("PATH", path_with(&[unmanaged.path(), &bin_dir]))
        .args(["update-local", "--managers", "cargo", "-o", "json"])
        .output()
        .expect("vership runs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a shadowed update is a failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
    let envelope: serde_json::Value =
        serde_json::from_str(last).expect("last stderr line must be the error envelope");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(message.contains("unmanaged"), "{message}");
    assert!(
        message.contains(unmanaged.path().to_str().unwrap()),
        "the message must name the copy that wins PATH: {message}"
    );
}

#[test]
fn a_shadowed_copy_is_only_judged_once_the_installs_have_run() {
    let root = project("vership-e2e-demo", "0.2.0");
    let unmanaged = TempDir::new().unwrap();
    write_executable(unmanaged.path(), "vership-e2e-demo");

    let run = |home: &Path, args: &[&str]| {
        let bin_dir = home.join("bin");
        write_executable(&bin_dir, "vership-e2e-demo");
        AssertCommand::cargo_bin("vership")
            .unwrap()
            .current_dir(root.path())
            .env("CARGO_HOME", home)
            .env_remove("CARGO_INSTALL_ROOT")
            .env("PATH", path_with(&[unmanaged.path(), &bin_dir]))
            .arg("update-local")
            .args(args)
            .output()
            .expect("vership runs")
    };

    // An install one version behind, under --dry-run: nothing has been
    // installed, so the copy that currently wins PATH is the plan's input.
    let outstanding = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.1.0",
        root.path(),
        "vership-e2e-demo",
    )]);
    let output = run(
        outstanding.path(),
        &["--dry-run", "--managers", "cargo", "-o", "text"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        !stdout.contains("FAIL"),
        "a run that has not tried cannot have failed: {stdout}"
    );
    assert!(
        stdout.contains("wait vership-e2e-demo"),
        "the unjudged copy is still reported: {stdout}"
    );

    // The control: with the install already at the target, nothing is
    // outstanding and the same shadowing is judged.
    let settled = cargo_home(&[path_entry(
        "vership-e2e-demo",
        "0.2.0",
        root.path(),
        "vership-e2e-demo",
    )]);
    let output = run(settled.path(), &["--managers", "cargo", "-o", "text"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("FAIL vership-e2e-demo"),
        "a settled run judges what wins PATH: {stdout}"
    );
}
