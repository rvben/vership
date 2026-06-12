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

use httpmock::prelude::*;
use vership::verify::CheckResult;
use vership::verify::checkers;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build()
}

#[test]
fn crates_found_when_exact_version_exists() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/api/v1/crates/mycrate/1.2.3")
            // crates.io rejects requests without a User-Agent; the checker
            // must always send one.
            .header_exists("user-agent");
        then.status(200).json_body(serde_json::json!({
            "version": {"num": "1.2.3"}
        }));
    });
    let result = checkers::crates(&agent(), &server.base_url(), "mycrate", "1.2.3");
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn crates_not_found_on_404() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/v1/crates/mycrate/9.9.9");
        then.status(404);
    });
    let result = checkers::crates(&agent(), &server.base_url(), "mycrate", "9.9.9");
    assert_eq!(result, CheckResult::NotFound);
}

#[test]
fn crates_server_error_is_check_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/v1/crates/mycrate/1.2.3");
        then.status(503);
    });
    let result = checkers::crates(&agent(), &server.base_url(), "mycrate", "1.2.3");
    assert!(matches!(result, CheckResult::Error(_)));
}

#[test]
fn pypi_found_when_exact_version_exists() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/pypi/mypkg/1.2.3/json");
        then.status(200)
            .json_body(serde_json::json!({"info": {"version": "1.2.3"}}));
    });
    let result = checkers::pypi(&agent(), &server.base_url(), "mypkg", "1.2.3");
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn pypi_not_found_on_404() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/pypi/mypkg/9.9.9/json");
        then.status(404);
    });
    let result = checkers::pypi(&agent(), &server.base_url(), "mypkg", "9.9.9");
    assert_eq!(result, CheckResult::NotFound);
}

#[test]
fn npm_found_when_exact_version_exists() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/mypkg/1.2.3");
        then.status(200)
            .json_body(serde_json::json!({"version": "1.2.3"}));
    });
    let result = checkers::npm(&agent(), &server.base_url(), "mypkg", "1.2.3");
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn npm_scoped_package_path_is_encoded() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/@rvben%2fmypkg/1.2.3");
        then.status(200)
            .json_body(serde_json::json!({"version": "1.2.3"}));
    });
    let result = checkers::npm(&agent(), &server.base_url(), "@rvben/mypkg", "1.2.3");
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn homebrew_found_when_formula_contains_version() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/rvben/homebrew-tap/HEAD/Formula/mytool.rb");
        then.status(200).body(
            "class Mytool < Formula\n  url \"https://github.com/rvben/mytool/releases/download/v1.2.3/mytool.tar.gz\"\nend\n",
        );
    });
    let result = checkers::homebrew(
        &agent(),
        &server.base_url(),
        "rvben/homebrew-tap",
        "mytool",
        "1.2.3",
    );
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn homebrew_old_version_is_found_old() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/rvben/homebrew-tap/HEAD/Formula/mytool.rb");
        then.status(200)
            .body("  version \"1.2.2\"\n  url \"...v1.2.2/mytool.tar.gz\"\n");
    });
    let result = checkers::homebrew(
        &agent(),
        &server.base_url(),
        "rvben/homebrew-tap",
        "mytool",
        "1.2.3",
    );
    assert_eq!(result, CheckResult::FoundOld("1.2.2".to_string()));
}

#[test]
fn homebrew_missing_formula_is_not_found() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/rvben/homebrew-tap/HEAD/Formula/mytool.rb");
        then.status(404);
    });
    let result = checkers::homebrew(
        &agent(),
        &server.base_url(),
        "rvben/homebrew-tap",
        "mytool",
        "1.2.3",
    );
    assert_eq!(result, CheckResult::NotFound);
}

#[test]
fn ghcr_found_when_tag_manifest_exists() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/token")
            .query_param("scope", "repository:rvben/myapp:pull");
        then.status(200)
            .json_body(serde_json::json!({"token": "anon-token"}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/rvben/myapp/manifests/1.2.3")
            .header("authorization", "Bearer anon-token");
        then.status(200).json_body(serde_json::json!({}));
    });
    let result = checkers::ghcr(&agent(), &server.base_url(), "rvben/myapp", "1.2.3");
    assert_eq!(result, CheckResult::Found("1.2.3".to_string()));
}

#[test]
fn ghcr_falls_back_to_v_prefixed_tag() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/token");
        then.status(200)
            .json_body(serde_json::json!({"token": "anon-token"}));
    });
    server.mock(|when, then| {
        when.method(GET).path("/v2/rvben/myapp/manifests/1.2.3");
        then.status(404);
    });
    server.mock(|when, then| {
        when.method(GET).path("/v2/rvben/myapp/manifests/v1.2.3");
        then.status(200).json_body(serde_json::json!({}));
    });
    let result = checkers::ghcr(&agent(), &server.base_url(), "rvben/myapp", "1.2.3");
    assert_eq!(result, CheckResult::Found("v1.2.3".to_string()));
}

#[test]
fn ghcr_both_tags_missing_is_not_found() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/token");
        then.status(200)
            .json_body(serde_json::json!({"token": "anon-token"}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path_matches(Regex::new("/v2/rvben/myapp/manifests/.*").unwrap());
        then.status(404);
    });
    let result = checkers::ghcr(&agent(), &server.base_url(), "rvben/myapp", "1.2.3");
    assert_eq!(result, CheckResult::NotFound);
}

#[test]
fn release_with_assets_is_found() {
    let body = serde_json::json!({"name": "v1.2.3", "assets": [{"name": "x.tar.gz"}]});
    assert_eq!(
        checkers::parse_release("1.2.3", &body),
        CheckResult::Found("1.2.3".to_string())
    );
}

#[test]
fn release_without_assets_is_error() {
    let body = serde_json::json!({"name": "v1.2.3", "assets": []});
    assert!(matches!(
        checkers::parse_release("1.2.3", &body),
        CheckResult::Error(_)
    ));
}
