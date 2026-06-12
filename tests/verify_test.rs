use std::fs;
use std::path::Path;

use vership::config::VerifyConfig;
use vership::verify::targets::{Target, detect_targets};

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn names(targets: &[Target]) -> Vec<&'static str> {
    targets.iter().map(|t| t.name()).collect()
}

#[test]
fn rust_repo_detects_tag_release_crates() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"mycrate\"\nversion = \"1.0.0\"\n",
    );
    let targets = detect_targets(
        dir.path(),
        &VerifyConfig::default(),
        Some("https://github.com/rvben/mycrate"),
    )
    .unwrap();
    assert_eq!(names(&targets), vec!["tag", "release", "crates"]);
    assert!(
        targets
            .iter()
            .any(|t| matches!(t, Target::Crates { name } if name == "mycrate"))
    );
}

#[test]
fn publish_false_skips_crates() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"internal\"\nversion = \"1.0.0\"\npublish = false\n",
    );
    let targets = detect_targets(
        dir.path(),
        &VerifyConfig::default(),
        Some("https://github.com/rvben/internal"),
    )
    .unwrap();
    assert!(!names(&targets).contains(&"crates"));
}

#[test]
fn pyproject_detects_pypi() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "pyproject.toml",
        "[project]\nname = \"mypkg\"\nversion = \"1.0.0\"\n",
    );
    let targets = detect_targets(dir.path(), &VerifyConfig::default(), None).unwrap();
    assert!(
        targets
            .iter()
            .any(|t| matches!(t, Target::Pypi { name } if name == "mypkg"))
    );
}

#[test]
fn private_package_json_skips_npm() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "package.json",
        r#"{"name": "internal-app", "version": "1.0.0", "private": true}"#,
    );
    let targets = detect_targets(dir.path(), &VerifyConfig::default(), None).unwrap();
    assert!(!names(&targets).contains(&"npm"));
}

#[test]
fn workflow_with_ghcr_detects_ghcr_with_default_image() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".github/workflows/release.yml",
        "jobs:\n  docker:\n    steps:\n      - run: docker push ghcr.io/rvben/MyApp:latest\n",
    );
    let targets = detect_targets(
        dir.path(),
        &VerifyConfig::default(),
        Some("https://github.com/rvben/MyApp"),
    )
    .unwrap();
    assert!(
        targets
            .iter()
            .any(|t| matches!(t, Target::Ghcr { image } if image == "rvben/myapp"))
    );
}

#[test]
fn workflow_with_homebrew_detects_tap_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".github/workflows/release.yml",
        "jobs:\n  brew:\n    steps:\n      - run: ./scripts/bump.sh rvben/homebrew-tap\n",
    );
    let targets = detect_targets(
        dir.path(),
        &VerifyConfig::default(),
        Some("https://github.com/rvben/mytool"),
    )
    .unwrap();
    assert!(targets.iter().any(
        |t| matches!(t, Target::Homebrew { tap, formula } if tap == "rvben/homebrew-tap" && formula == "mytool")
    ));
}

#[test]
fn config_skip_removes_target() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"mycrate\"\nversion = \"1.0.0\"\n",
    );
    let config = VerifyConfig {
        skip: vec!["crates".to_string()],
        ..Default::default()
    };
    let targets = detect_targets(
        dir.path(),
        &config,
        Some("https://github.com/rvben/mycrate"),
    )
    .unwrap();
    assert!(!names(&targets).contains(&"crates"));
}

#[test]
fn no_remote_means_no_tag_release_targets() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"mycrate\"\nversion = \"1.0.0\"\n",
    );
    let targets = detect_targets(dir.path(), &VerifyConfig::default(), None).unwrap();
    assert_eq!(names(&targets), vec!["crates"]);
}

#[test]
fn cli_filters_apply_only_then_skip() {
    use vership::verify::targets::filter_targets;
    let targets = vec![
        Target::Tag,
        Target::Release,
        Target::Crates {
            name: "x".to_string(),
        },
    ];
    let filtered = filter_targets(targets, Some("tag,crates"), Some("tag")).unwrap();
    assert_eq!(names(&filtered), vec!["crates"]);
}

#[test]
fn cli_filter_rejects_unknown_target() {
    use vership::verify::targets::filter_targets;
    assert!(filter_targets(vec![Target::Tag], Some("cargo"), None).is_err());
}
