use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::VersionFileEntry;
use crate::error::{Error, Result};

/// Apply all version_files entries, returning the list of files modified (relative paths).
pub fn apply(
    root: &Path,
    entries: &[VersionFileEntry],
    prev: &str,
    version: &str,
) -> Result<Vec<PathBuf>> {
    let mut touched = BTreeSet::new();

    for entry in entries {
        if entry.field.is_some() && (entry.search.is_some() || entry.replace.is_some()) {
            return Err(Error::Config(format!(
                "version_files entry for '{}': 'field' and 'search'/'replace' are mutually exclusive",
                entry.glob
            )));
        } else if entry.field.is_some() {
            apply_field_mode(root, entry, version, &mut touched)?;
        } else if let (Some(search), Some(replace)) = (&entry.search, &entry.replace) {
            apply_text_mode(root, entry, search, replace, prev, version, &mut touched)?;
        } else {
            return Err(Error::Config(format!(
                "version_files entry for '{}': must have either 'field' or both 'search' and 'replace'",
                entry.glob
            )));
        }
    }

    Ok(touched.into_iter().collect())
}

fn apply_text_mode(
    root: &Path,
    entry: &VersionFileEntry,
    search_template: &str,
    replace_template: &str,
    prev: &str,
    version: &str,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let search = search_template.replace("{prev}", prev);
    let replace = replace_template.replace("{version}", version);

    let pattern = root.join(&entry.glob).to_string_lossy().into_owned();
    let paths = glob::glob(&pattern)
        .map_err(|e| Error::Config(format!("invalid glob '{}': {e}", entry.glob)))?;

    for path_result in paths {
        let path = path_result.map_err(|e| Error::Other(format!("glob error: {e}")))?;

        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read {}: {e}", path.display())))?;

        if !content.contains(&search) {
            continue;
        }

        let updated = content.replace(&search, &replace);
        std::fs::write(&path, &updated)
            .map_err(|e| Error::Other(format!("write {}: {e}", path.display())))?;

        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        touched.insert(relative);
    }

    Ok(())
}

fn apply_field_mode(
    root: &Path,
    entry: &VersionFileEntry,
    version: &str,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let field = entry.field.as_deref().unwrap();

    let pattern = root.join(&entry.glob).to_string_lossy().into_owned();
    let paths = glob::glob(&pattern)
        .map_err(|e| Error::Config(format!("invalid glob '{}': {e}", entry.glob)))?;

    for path_result in paths {
        let path = path_result.map_err(|e| Error::Other(format!("glob error: {e}")))?;

        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Other(format!("read {}: {e}", path.display())))?;

        let mut json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| Error::Other(format!("parse JSON {}: {e}", path.display())))?;

        update_json_field(&mut json, field, version, &path)?;

        let output = format_json(&json);
        std::fs::write(&path, &output)
            .map_err(|e| Error::Other(format!("write {}: {e}", path.display())))?;

        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        touched.insert(relative);
    }

    Ok(())
}

fn update_json_field(
    json: &mut serde_json::Value,
    field: &str,
    version: &str,
    path: &Path,
) -> Result<()> {
    let parts: Vec<&str> = field.split('.').collect();

    if parts.last() == Some(&"*") {
        let parent_path = &parts[..parts.len() - 1];
        let obj = navigate_to_mut(json, parent_path, path)?;
        let map = obj.as_object_mut().ok_or_else(|| {
            Error::Other(format!(
                "{}: field '{}' is not an object",
                path.display(),
                parent_path.join(".")
            ))
        })?;
        for value in map.values_mut() {
            *value = serde_json::Value::String(version.to_string());
        }
    } else {
        let parent_path = &parts[..parts.len() - 1];
        let leaf = *parts.last().unwrap();
        let obj = if parent_path.is_empty() {
            json
        } else {
            navigate_to_mut(json, parent_path, path)?
        };
        let location = if parent_path.is_empty() {
            "root".to_string()
        } else {
            parts[..parts.len() - 1].join(".")
        };
        let map = obj.as_object_mut().ok_or_else(|| {
            Error::Other(format!(
                "{}: cannot index into non-object at '{location}'",
                path.display(),
            ))
        })?;
        if !map.contains_key(leaf) {
            return Err(Error::Other(format!(
                "{}: field '{}' not found",
                path.display(),
                field
            )));
        }
        map.insert(
            leaf.to_string(),
            serde_json::Value::String(version.to_string()),
        );
    }

    Ok(())
}

fn navigate_to_mut<'a>(
    json: &'a mut serde_json::Value,
    path: &[&str],
    file_path: &Path,
) -> Result<&'a mut serde_json::Value> {
    let mut current = json;
    for &segment in path {
        current = current.get_mut(segment).ok_or_else(|| {
            Error::Other(format!(
                "{}: field '{}' not found",
                file_path.display(),
                segment
            ))
        })?;
    }
    Ok(current)
}

/// Format JSON with 2-space indent and trailing newline (npm convention).
fn format_json(value: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(value).expect("serialize JSON");
    format!("{pretty}\n")
}
