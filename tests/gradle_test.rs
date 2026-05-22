use std::fs;

use semver::Version;
use tempfile::TempDir;
use vership::project::ProjectType;
use vership::project::gradle::GradleProject;

#[test]
fn gradle_name() {
    let project = GradleProject::new();
    assert_eq!(project.name(), "Gradle");
}

#[test]
fn gradle_is_file_versioned() {
    let project = GradleProject::new();
    assert!(!project.is_tag_versioned());
}

#[test]
fn gradle_read_plugin_version_from_properties() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("gradle.properties"),
        "pluginGroup=com.example\npluginVersion=0.1.4\npluginSinceBuild=252\n",
    )
    .unwrap();

    let project = GradleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 1, 4));
}

#[test]
fn gradle_read_version_from_properties() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("gradle.properties"),
        "group=com.example\nversion=2.3.4\n",
    )
    .unwrap();

    let project = GradleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(2, 3, 4));
}

#[test]
fn gradle_read_version_from_build_script() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("build.gradle.kts"),
        "plugins { id(\"java\") }\nversion = \"1.2.3\"\n",
    )
    .unwrap();

    let project = GradleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(1, 2, 3));
}

#[test]
fn gradle_plugin_version_takes_priority_over_version_key() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("gradle.properties"),
        "version=9.9.9\npluginVersion=0.1.4\n",
    )
    .unwrap();

    let project = GradleProject::new();
    let version = project.read_version(dir.path()).unwrap();
    assert_eq!(version, Version::new(0, 1, 4));
}

#[test]
fn gradle_write_plugin_version_preserves_other_keys() {
    let dir = TempDir::new().unwrap();
    let props = dir.path().join("gradle.properties");
    fs::write(
        &props,
        "pluginGroup=com.example\npluginVersion=0.1.4\npluginSinceBuild=252\n",
    )
    .unwrap();

    let project = GradleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 1, 5))
        .unwrap();

    let content = fs::read_to_string(&props).unwrap();
    assert!(content.contains("pluginVersion=0.1.5"));
    assert!(content.contains("pluginGroup=com.example"));
    assert!(content.contains("pluginSinceBuild=252"));
    assert!(!content.contains("0.1.4"));
}

#[test]
fn gradle_write_does_not_touch_plugin_prefixed_version() {
    let dir = TempDir::new().unwrap();
    let props = dir.path().join("gradle.properties");
    fs::write(&props, "pluginVersion=0.1.4\nversion=2.0.0\n").unwrap();

    let project = GradleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 1, 5))
        .unwrap();

    let content = fs::read_to_string(&props).unwrap();
    assert!(content.contains("pluginVersion=0.1.5"));
    // The standalone `version` key must be left untouched when pluginVersion owns the version.
    assert!(content.contains("version=2.0.0"));
}

#[test]
fn gradle_write_version_in_build_script() {
    let dir = TempDir::new().unwrap();
    let script = dir.path().join("build.gradle.kts");
    fs::write(
        &script,
        "plugins { id(\"java\") }\nversion = \"1.2.3\"\ngroup = \"com.example\"\n",
    )
    .unwrap();

    let project = GradleProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 3, 0))
        .unwrap();

    let content = fs::read_to_string(&script).unwrap();
    assert!(content.contains("version = \"1.3.0\""));
    assert!(content.contains("group = \"com.example\""));
}

#[test]
fn gradle_modified_files_reports_properties() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("gradle.properties"),
        "pluginVersion=0.1.4\n",
    )
    .unwrap();

    let project = GradleProject::new();
    project
        .write_version(dir.path(), &Version::new(0, 1, 5))
        .unwrap();

    let files = project.modified_files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].to_str(), Some("gradle.properties"));
}

#[test]
fn gradle_modified_files_reports_build_script() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("build.gradle.kts"), "version = \"1.0.0\"\n").unwrap();

    let project = GradleProject::new();
    project
        .write_version(dir.path(), &Version::new(1, 0, 1))
        .unwrap();

    let files = project.modified_files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].to_str(), Some("build.gradle.kts"));
}

#[test]
fn gradle_read_version_errors_when_absent() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("build.gradle.kts"),
        "plugins { id(\"java\") }\n",
    )
    .unwrap();

    let project = GradleProject::new();
    assert!(project.read_version(dir.path()).is_err());
}
