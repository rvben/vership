//! Rewrites intra-workspace dependency version requirements when a Cargo
//! workspace's shared version bumps.
//!
//! vership models a Cargo workspace as a single shared version: it reads and
//! writes exactly one `[workspace.package].version`. When a member manifest
//! depends on a sibling member via `sib = { path = "../sib", version = "X" }`,
//! that version requirement must move to the same new version, or the
//! sibling becomes unresolvable after a minor/major bump (a `^X.Y.Z`
//! requirement stops matching once the sibling crosses the next breaking
//! boundary). This module discovers workspace member package names and
//! rewrites their version requirements everywhere they appear as a
//! dependency, using `toml_edit` so unrelated formatting, comments, and key
//! order survive the rewrite.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use crate::error::{Error, Result};

const DEP_TABLE_KEYS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Rewrite `version` requirements on intra-workspace dependencies to
/// `new_version`. Returns the manifest paths that were actually changed,
/// relative to `root` and deduplicated. Returns `Ok(vec![])` when
/// `root/Cargo.toml` has no `[workspace]` table (single-crate project;
/// nothing to do).
pub fn update_intra_workspace_dep_versions(
    root: &Path,
    new_version: &semver::Version,
) -> Result<Vec<PathBuf>> {
    let root_manifest_path = root.join("Cargo.toml");
    let root_doc = read_document(&root_manifest_path)?;

    let Some(workspace) = root_doc.get("workspace").and_then(Item::as_table) else {
        return Ok(vec![]);
    };

    let member_dirs = discover_member_dirs(root, workspace)?;

    let mut member_names: BTreeSet<String> = BTreeSet::new();
    let mut manifest_paths: Vec<PathBuf> = Vec::new();

    for dir in &member_dirs {
        let manifest = dir.join("Cargo.toml");
        if let Some(name) = read_package_name(&manifest)? {
            member_names.insert(name);
        }
        manifest_paths.push(manifest);
    }

    // The root manifest may itself carry a [package] table (a workspace root
    // that is also a crate), and may carry [workspace.dependencies].
    if let Some(name) = read_package_name(&root_manifest_path)? {
        member_names.insert(name);
    }
    manifest_paths.push(root_manifest_path.clone());

    let mut changed: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for manifest_path in manifest_paths {
        if !seen.insert(manifest_path.clone()) {
            continue;
        }
        if update_manifest_file(&manifest_path, &member_names, new_version)? {
            let rel = manifest_path
                .strip_prefix(root)
                .unwrap_or(&manifest_path)
                .to_path_buf();
            if !changed.contains(&rel) {
                changed.push(rel);
            }
        }
    }

    Ok(changed)
}

fn read_document(manifest_path: &Path) -> Result<DocumentMut> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| Error::Other(format!("read {}: {e}", manifest_path.display())))?;
    content
        .parse::<DocumentMut>()
        .map_err(|e| Error::Other(format!("parse {}: {e}", manifest_path.display())))
}

/// Expand `[workspace].members` glob patterns (relative to `root`) into
/// existing member directories. A literal entry like `"core"` resolves to
/// that directory; a pattern like `"crates/*"` expands normally.
fn discover_member_dirs(root: &Path, workspace: &toml_edit::Table) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();

    let Some(members) = workspace.get("members").and_then(Item::as_array) else {
        return Ok(dirs);
    };

    for value in members.iter() {
        let Some(pattern) = value.as_str() else {
            continue;
        };
        let full_pattern = root.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy().into_owned();
        let paths = glob::glob(&full_pattern_str)
            .map_err(|e| Error::Other(format!("invalid workspace member glob '{pattern}': {e}")))?;
        for path_result in paths {
            let path =
                path_result.map_err(|e| Error::Other(format!("workspace member glob: {e}")))?;
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }

    Ok(dirs)
}

fn read_package_name(manifest_path: &Path) -> Result<Option<String>> {
    if !manifest_path.exists() {
        return Ok(None);
    }
    let doc = read_document(manifest_path)?;
    Ok(doc
        .get("package")
        .and_then(Item::as_table)
        .and_then(|t| t.get("name"))
        .and_then(Item::as_str)
        .map(str::to_string))
}

/// Scan and rewrite every dependency table in a single manifest. Returns
/// whether the manifest was actually changed.
fn update_manifest_file(
    manifest_path: &Path,
    member_names: &BTreeSet<String>,
    new_version: &semver::Version,
) -> Result<bool> {
    let mut doc = read_document(manifest_path)?;
    let mut changed = false;

    for key in DEP_TABLE_KEYS {
        if update_dep_table(doc.get_mut(key), member_names, new_version) {
            changed = true;
        }
    }

    if let Some(target_table) = doc.get_mut("target").and_then(Item::as_table_like_mut) {
        let cfg_keys: Vec<String> = target_table.iter().map(|(k, _)| k.to_string()).collect();
        for cfg in cfg_keys {
            let Some(cfg_table) = target_table.get_mut(&cfg).and_then(Item::as_table_like_mut)
            else {
                continue;
            };
            for key in DEP_TABLE_KEYS {
                if update_dep_table(cfg_table.get_mut(key), member_names, new_version) {
                    changed = true;
                }
            }
        }
    }

    if let Some(workspace_table) = doc.get_mut("workspace").and_then(Item::as_table_like_mut)
        && update_dep_table(
            workspace_table.get_mut("dependencies"),
            member_names,
            new_version,
        )
    {
        changed = true;
    }

    if changed {
        std::fs::write(manifest_path, doc.to_string())
            .map_err(|e| Error::Other(format!("write {}: {e}", manifest_path.display())))?;
    }

    Ok(changed)
}

fn update_dep_table(
    item: Option<&mut Item>,
    member_names: &BTreeSet<String>,
    new_version: &semver::Version,
) -> bool {
    let Some(table) = item.and_then(Item::as_table_like_mut) else {
        return false;
    };

    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    let mut changed = false;
    for key in keys {
        if let Some(entry) = table.get_mut(&key)
            && update_dep_entry(&key, entry, member_names, new_version)
        {
            changed = true;
        }
    }
    changed
}

/// Rewrite a single dependency entry's version requirement, if it names a
/// workspace member and carries a version to rewrite.
///
/// - `key = { ..., version = "X", ... }` (table or inline table): the
///   `version` key is replaced, all other keys and formatting untouched.
/// - `key = "X"` (bare string requirement): the whole value is replaced.
/// - a table-like entry with no `version` key (path-only dependency, or
///   `{ workspace = true }` inheriting from `[workspace.dependencies]`) is
///   left untouched: there is nothing to rewrite.
/// - the effective crate name is the entry's `package = "..."` rename field
///   when present, else the key itself.
fn update_dep_entry(
    key: &str,
    item: &mut Item,
    member_names: &BTreeSet<String>,
    new_version: &semver::Version,
) -> bool {
    if let Some(table) = item.as_table_like_mut() {
        let effective_name = table
            .get("package")
            .and_then(Item::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| key.to_string());

        if !member_names.contains(&effective_name) {
            return false;
        }

        let Some(version_value) = table.get_mut("version").and_then(Item::as_value_mut) else {
            return false;
        };
        set_version(version_value, new_version);
        return true;
    }

    if !member_names.contains(key) {
        return false;
    }

    let Some(value) = item.as_value_mut() else {
        return false;
    };
    if !value.is_str() {
        return false;
    }
    set_version(value, new_version);
    true
}

/// Replace a scalar string value in place, preserving its surrounding
/// whitespace/comment decoration.
fn set_version(value: &mut Value, new_version: &semver::Version) {
    let decor = value.decor().clone();
    let mut new_value = Value::from(new_version.to_string());
    *new_value.decor_mut() = decor;
    *value = new_value;
}
