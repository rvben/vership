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
