use std::fs;
use tempfile::TempDir;
use vership::artifacts;
use vership::config::ArtifactEntry;

#[test]
fn artifact_captures_stdout_to_output_file() {
    let dir = TempDir::new().unwrap();

    let entries = vec![ArtifactEntry {
        command: "echo hello".to_string(),
        output: Some("out.txt".to_string()),
        files: vec![],
    }];

    let produced = artifacts::run(dir.path(), &entries).unwrap();
    assert_eq!(produced, vec![std::path::PathBuf::from("out.txt")]);

    let content = fs::read_to_string(dir.path().join("out.txt")).unwrap();
    assert_eq!(content, "hello\n");
}

#[test]
fn artifact_collects_files_from_self_writing_command() {
    let dir = TempDir::new().unwrap();

    let entries = vec![ArtifactEntry {
        command: "echo data > generated.json".to_string(),
        output: None,
        files: vec!["generated.json".to_string()],
    }];

    let produced = artifacts::run(dir.path(), &entries).unwrap();
    assert_eq!(produced, vec![std::path::PathBuf::from("generated.json")]);
}

#[test]
fn artifact_command_failure_returns_error() {
    let dir = TempDir::new().unwrap();

    let entries = vec![ArtifactEntry {
        command: "false".to_string(),
        output: Some("out.txt".to_string()),
        files: vec![],
    }];

    let result = artifacts::run(dir.path(), &entries);
    assert!(result.is_err());
}

#[test]
fn artifact_multiple_entries_collects_all_files() {
    let dir = TempDir::new().unwrap();

    let entries = vec![
        ArtifactEntry {
            command: "echo a".to_string(),
            output: Some("a.txt".to_string()),
            files: vec![],
        },
        ArtifactEntry {
            command: "echo b > b.txt".to_string(),
            output: None,
            files: vec!["b.txt".to_string()],
        },
    ];

    let produced = artifacts::run(dir.path(), &entries).unwrap();
    assert_eq!(produced.len(), 2);
}

#[test]
fn artifact_no_output_or_files_still_runs() {
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("ran");

    let entries = vec![ArtifactEntry {
        command: format!("touch {}", marker.display()),
        output: None,
        files: vec![],
    }];

    let produced = artifacts::run(dir.path(), &entries).unwrap();
    assert!(produced.is_empty());
    assert!(marker.exists());
}
