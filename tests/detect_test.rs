use std::fs;

use tempfile::TempDir;
use vership::project;

#[test]
fn detect_rust_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_rust_maturin_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[build-system]\nrequires = [\"maturin\"]\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust + Maturin");
}

#[test]
fn detect_node_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_python_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Python");
}

#[test]
fn detect_rust_over_node() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_node_over_python() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_override_node() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("node")).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_override_python() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("python")).unwrap();
    assert_eq!(p.name(), "Python");
}

#[test]
fn detect_go_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("go.mod"),
        "module example.com/test\n\ngo 1.21\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Go");
}

#[test]
fn detect_rust_over_go() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_node_over_go() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_go_over_python() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Go");
}

#[test]
fn detect_override_go() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("go")).unwrap();
    assert_eq!(p.name(), "Go");
}

#[test]
fn detect_gradle_kts_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("build.gradle.kts"),
        "plugins { id(\"java\") }\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Gradle");
}

#[test]
fn detect_gradle_groovy_project() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("build.gradle"), "version = '1.0.0'\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Gradle");
}

#[test]
fn detect_gradle_settings_only_project() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Gradle");
}

#[test]
fn detect_rust_over_gradle() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("build.gradle.kts"), "version = \"1.0.0\"\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Rust");
}

#[test]
fn detect_override_gradle() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("gradle")).unwrap();
    assert_eq!(p.name(), "Gradle");
}

#[test]
fn detect_unknown_override() {
    let dir = TempDir::new().unwrap();
    let result = project::detect(dir.path(), Some("java"));
    assert!(result.is_err());
}

#[test]
fn detect_no_project() {
    let dir = TempDir::new().unwrap();
    let result = project::detect(dir.path(), None);
    assert!(result.is_err());
}

#[test]
fn detect_ansible_collection() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("galaxy.yml"),
        "namespace: hda\nname: platform\nversion: \"0.0.2\"\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Ansible Collection");
}

#[test]
fn detect_ansible_wins_over_tooling_pyproject() {
    // A collection repo carrying a tooling-only pyproject.toml (ruff/ansible-lint)
    // must resolve to the Ansible collection, not Python.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("galaxy.yml"),
        "namespace: hda\nname: platform\nversion: \"0.0.2\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.ruff]\nline-length = 100\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Ansible Collection");
}

#[test]
fn detect_galaxy_yml_without_identity_keys_is_not_a_collection() {
    // namespace + name form a collection's identity. A galaxy.yml without them
    // is not enough to claim the Ansible type; detection must fall through.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("galaxy.yml"), "version: \"0.0.2\"\n").unwrap();
    let result = project::detect(dir.path(), None);
    assert!(result.is_err());
}

#[test]
fn detect_ansible_collection_without_version_still_detects() {
    // A collection whose version is missing or malformed is still an Ansible
    // collection: detect it so read_version can report a precise error rather
    // than the misleading "no supported project type detected".
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("galaxy.yml"),
        "namespace: hda\nname: platform\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Ansible Collection");
}

#[test]
fn detect_override_ansible_collection() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("ansible-collection")).unwrap();
    assert_eq!(p.name(), "Ansible Collection");
}

#[test]
fn detect_private_package_json_loses_to_go_mod() {
    // The shape that broke a real release: a Go repo grew a private Playwright
    // harness at the root. package.json outranked go.mod, so the version was
    // read from a manifest that has none and every bump failed at tag time,
    // with nothing in CI to catch it first.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "app-browser-tests", "private": true, "devDependencies": {"@playwright/test": "1.62.1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Go");
}

#[test]
fn detect_private_package_json_loses_to_python() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "docs", "private": true}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Python");
}

#[test]
fn detect_private_package_json_alone_is_still_node() {
    // A private application is not published, but it is still versioned in
    // package.json. Skipping it entirely would leave such a repo undetectable.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "internal-app", "private": true, "version": "1.0.0"}"#,
    )
    .unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_published_package_json_still_wins_over_go_mod() {
    // Only `private` demotes a manifest. A publishable package.json alongside a
    // go.mod keeps the existing precedence.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_private_false_package_json_wins_over_go_mod() {
    // `"private": false` is an explicit statement that the package is
    // publishable, so it must behave like an absent key, not like `true`.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test", "private": false, "version": "1.0.0"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_malformed_package_json_still_claims_the_repo() {
    // An unparseable manifest is not evidence of tooling. Keep claiming the
    // repo so read_version reports a precise parse error rather than silently
    // releasing something else's version.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("package.json"), "{ not json").unwrap();
    fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();
    let p = project::detect(dir.path(), None).unwrap();
    assert_eq!(p.name(), "Node");
}

#[test]
fn detect_override_ansible_alias() {
    let dir = TempDir::new().unwrap();
    let p = project::detect(dir.path(), Some("ansible")).unwrap();
    assert_eq!(p.name(), "Ansible Collection");
}
