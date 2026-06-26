use std::fs;

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::ansible::AnsibleProject;

fn write_galaxy(dir: &TempDir, content: &str) {
    fs::write(dir.path().join("galaxy.yml"), content).unwrap();
}

#[test]
fn ansible_name() {
    let project = AnsibleProject::new();
    assert_eq!(project.name(), "Ansible Collection");
}

#[test]
fn ansible_is_file_versioned() {
    let project = AnsibleProject::new();
    assert!(!project.is_tag_versioned());
}

#[test]
fn ansible_publishes_only_git_tag() {
    // Collections are consumed by git ref; the tag is the whole release, so
    // verify defaults to tag-only (no GitHub Release or registry targets).
    let project = AnsibleProject::new();
    assert!(project.publishes_only_git_tag());
}

#[test]
fn ansible_read_quoted_version() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: \"0.0.2\"\n");

    let project = AnsibleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 0, 2));
}

#[test]
fn ansible_read_unquoted_version() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: 0.0.2\n");

    let project = AnsibleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 0, 2));
}

#[test]
fn ansible_write_preserves_quotes_and_other_keys() {
    let dir = TempDir::new().unwrap();
    let galaxy =
        "# managed\nnamespace: hda\nname: platform\nversion: \"0.0.2\"\nreadme: README.md\n";
    write_galaxy(&dir, galaxy);

    let project = AnsibleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 0, 3))
        .unwrap();

    let content = fs::read_to_string(dir.path().join("galaxy.yml")).unwrap();
    assert_eq!(
        content,
        "# managed\nnamespace: hda\nname: platform\nversion: \"0.0.3\"\nreadme: README.md\n"
    );
}

#[test]
fn ansible_write_keeps_unquoted_unquoted() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: 0.0.2\n");

    let project = AnsibleProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 0))
        .unwrap();

    let content = fs::read_to_string(dir.path().join("galaxy.yml")).unwrap();
    assert!(content.contains("version: 1.0.0\n"));
    assert!(!content.contains("\"1.0.0\""));
}

#[test]
fn ansible_write_changes_only_the_version_line() {
    let dir = TempDir::new().unwrap();
    let galaxy = "# header comment\nname: platform\nnamespace: hda\nversion: \"0.0.2\"  # inline\nauthors:\n  - Someone\n";
    write_galaxy(&dir, galaxy);

    let project = AnsibleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 1, 0))
        .unwrap();

    let content = fs::read_to_string(dir.path().join("galaxy.yml")).unwrap();
    // Exactly one line differs from the original.
    let changed: Vec<_> = galaxy
        .lines()
        .zip(content.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].1, "version: \"0.1.0\"  # inline");
}

#[test]
fn ansible_modified_files_reports_galaxy() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: \"0.0.2\"\n");

    let project = AnsibleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 0, 3))
        .unwrap();

    let files = project.modified_files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].to_str(), Some("galaxy.yml"));
}

#[test]
fn ansible_read_version_errors_when_absent() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\n");

    let project = AnsibleProject::new();
    assert!(project.read_version(dir.path()).is_err());
}

#[test]
fn ansible_read_version_errors_when_not_semver() {
    let dir = TempDir::new().unwrap();
    write_galaxy(
        &dir,
        "namespace: hda\nname: platform\nversion: \"not-a-version\"\n",
    );

    let project = AnsibleProject::new();
    assert!(project.read_version(dir.path()).is_err());
}

#[test]
fn ansible_read_version_errors_on_unterminated_quote() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: \"0.0.2\n");

    let project = AnsibleProject::new();
    assert!(project.read_version(dir.path()).is_err());
}

#[test]
fn ansible_write_does_not_mutate_on_missing_version() {
    let dir = TempDir::new().unwrap();
    let galaxy = "namespace: hda\nname: platform\n";
    write_galaxy(&dir, galaxy);

    let project = AnsibleProject::new();
    assert!(
        project
            .write_version(dir.path(), &Version::new(0, 0, 3))
            .is_err()
    );
    let content = fs::read_to_string(dir.path().join("galaxy.yml")).unwrap();
    assert_eq!(content, galaxy);
}

#[test]
fn ansible_package_name_is_fqcn() {
    let dir = TempDir::new().unwrap();
    write_galaxy(&dir, "namespace: hda\nname: platform\nversion: \"0.0.2\"\n");

    let project = AnsibleProject::new();
    assert_eq!(
        project.package_name(dir.path()).unwrap().as_deref(),
        Some("hda.platform")
    );
}
