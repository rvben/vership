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
        if entry.field.is_some() {
            todo!("field mode")
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
        let path = path_result
            .map_err(|e| Error::Other(format!("glob error: {e}")))?;

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
