use std::fs;
use tempfile::TempDir;
use vership::config::VersionFileEntry;
use vership::version_files;

#[test]
fn text_mode_replaces_prev_with_version() {
    let dir = TempDir::new().unwrap();
    let readme = dir.path().join("README.md");
    fs::write(&readme, "Install: rev: v1.2.3\nAlso: mise use tool@1.2.3\n").unwrap();

    let entries = vec![
        VersionFileEntry {
            glob: "README.md".to_string(),
            search: Some("rev: v{prev}".to_string()),
            replace: Some("rev: v{version}".to_string()),
            field: None,
        },
        VersionFileEntry {
            glob: "README.md".to_string(),
            search: Some("tool@{prev}".to_string()),
            replace: Some("tool@{version}".to_string()),
            field: None,
        },
    ];

    let touched = version_files::apply(dir.path(), &entries, "1.2.3", "1.3.0").unwrap();

    let content = fs::read_to_string(&readme).unwrap();
    assert_eq!(content, "Install: rev: v1.3.0\nAlso: mise use tool@1.3.0\n");
    assert!(touched.contains(&"README.md".into()));
}

#[test]
fn text_mode_skips_file_without_match() {
    let dir = TempDir::new().unwrap();
    let readme = dir.path().join("README.md");
    fs::write(&readme, "No version here\n").unwrap();

    let entries = vec![VersionFileEntry {
        glob: "README.md".to_string(),
        search: Some("rev: v{prev}".to_string()),
        replace: Some("rev: v{version}".to_string()),
        field: None,
    }];

    let touched = version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0").unwrap();
    assert!(touched.is_empty());

    let content = fs::read_to_string(&readme).unwrap();
    assert_eq!(content, "No version here\n");
}

#[test]
fn text_mode_glob_matches_multiple_files() {
    let dir = TempDir::new().unwrap();
    let docs = dir.path().join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("a.md"), "rev: v2.0.0\n").unwrap();
    fs::write(docs.join("b.md"), "rev: v2.0.0\n").unwrap();

    let entries = vec![VersionFileEntry {
        glob: "docs/*.md".to_string(),
        search: Some("rev: v{prev}".to_string()),
        replace: Some("rev: v{version}".to_string()),
        field: None,
    }];

    let touched = version_files::apply(dir.path(), &entries, "2.0.0", "2.1.0").unwrap();
    assert_eq!(touched.len(), 2);

    assert_eq!(fs::read_to_string(docs.join("a.md")).unwrap(), "rev: v2.1.0\n");
    assert_eq!(fs::read_to_string(docs.join("b.md")).unwrap(), "rev: v2.1.0\n");
}

#[test]
fn field_mode_updates_json_version() {
    let dir = TempDir::new().unwrap();
    let pkg = dir.path().join("package.json");
    fs::write(&pkg, "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}\n").unwrap();

    let entries = vec![VersionFileEntry {
        glob: "package.json".to_string(),
        search: None,
        replace: None,
        field: Some("version".to_string()),
    }];

    let touched = version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0").unwrap();
    assert_eq!(touched.len(), 1);

    let content = fs::read_to_string(&pkg).unwrap();
    assert!(content.contains("\"version\": \"1.1.0\""));
    assert!(content.contains("\"name\": \"test\""));
}

#[test]
fn field_mode_wildcard_updates_all_values() {
    let dir = TempDir::new().unwrap();
    let pkg = dir.path().join("package.json");
    fs::write(
        &pkg,
        "{\n  \"optionalDependencies\": {\n    \"pkg-a\": \"1.0.0\",\n    \"pkg-b\": \"1.0.0\"\n  }\n}\n",
    )
    .unwrap();

    let entries = vec![VersionFileEntry {
        glob: "package.json".to_string(),
        search: None,
        replace: None,
        field: Some("optionalDependencies.*".to_string()),
    }];

    let touched = version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0").unwrap();
    assert_eq!(touched.len(), 1);

    let content = fs::read_to_string(&pkg).unwrap();
    assert!(content.contains("\"pkg-a\": \"1.1.0\""));
    assert!(content.contains("\"pkg-b\": \"1.1.0\""));
}

#[test]
fn field_mode_glob_matches_multiple_json_files() {
    let dir = TempDir::new().unwrap();
    let npm = dir.path().join("npm");
    let cli_a = npm.join("cli-a");
    let cli_b = npm.join("cli-b");
    fs::create_dir_all(&cli_a).unwrap();
    fs::create_dir_all(&cli_b).unwrap();
    fs::write(cli_a.join("package.json"), "{\"version\": \"1.0.0\"}\n").unwrap();
    fs::write(cli_b.join("package.json"), "{\"version\": \"1.0.0\"}\n").unwrap();

    let entries = vec![VersionFileEntry {
        glob: "npm/*/package.json".to_string(),
        search: None,
        replace: None,
        field: Some("version".to_string()),
    }];

    let touched = version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0").unwrap();
    assert_eq!(touched.len(), 2);
}

#[test]
fn field_mode_missing_field_returns_error() {
    let dir = TempDir::new().unwrap();
    let pkg = dir.path().join("package.json");
    fs::write(&pkg, "{\"name\": \"test\"}\n").unwrap();

    let entries = vec![VersionFileEntry {
        glob: "package.json".to_string(),
        search: None,
        replace: None,
        field: Some("version".to_string()),
    }];

    let result = version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0");
    assert!(result.is_err());
}

#[test]
fn field_mode_preserves_2_space_indent_with_trailing_newline() {
    let dir = TempDir::new().unwrap();
    let pkg = dir.path().join("package.json");
    let original = "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}\n";
    fs::write(&pkg, original).unwrap();

    let entries = vec![VersionFileEntry {
        glob: "package.json".to_string(),
        search: None,
        replace: None,
        field: Some("version".to_string()),
    }];

    version_files::apply(dir.path(), &entries, "1.0.0", "1.1.0").unwrap();

    let content = fs::read_to_string(&pkg).unwrap();
    assert!(content.starts_with("{\n  "));
    assert!(content.ends_with("}\n"));
    assert!(!content.contains("    ")); // no 4-space indent
}
