use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub message: String,
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run git and return stdout if successful, or None if git exits non-zero.
/// Use this for commands where a non-zero exit means "not found" rather than an error.
fn git_output_optional(root: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn git_success(root: &Path, args: &[&str]) -> Result<bool> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    Ok(status.success())
}

/// Return the latest semver tag (sorted by version), or None if no tags exist.
pub fn latest_semver_tag(root: &Path) -> Result<Option<String>> {
    let output = git_output(root, &["tag", "--sort=-v:refname", "-l", "v*"])?;
    if output.is_empty() {
        return Ok(None);
    }
    Ok(output.lines().next().map(|s| s.to_string()))
}

/// Check whether the given tag exists in the repository.
pub fn tag_exists(root: &Path, tag: &str) -> Result<bool> {
    git_success(
        root,
        &["rev-parse", "--verify", &format!("refs/tags/{tag}")],
    )
}

/// Return true if the working tree has staged or unstaged changes, including untracked files.
pub fn has_uncommitted_changes(root: &Path) -> Result<bool> {
    // Check staged and unstaged changes to tracked files only.
    // Untracked files should not block a release.
    let has_staged = !git_success(root, &["diff", "--cached", "--quiet"])?;
    let has_unstaged = !git_success(root, &["diff", "--quiet"])?;
    Ok(has_staged || has_unstaged)
}

/// Return the name of the currently checked-out branch.
pub fn current_branch(root: &Path) -> Result<String> {
    let branch = git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch.is_empty() {
        return Err(Error::Git("could not determine current branch".to_string()));
    }
    Ok(branch)
}

/// Return all commits reachable from HEAD since the given tag.
/// When `tag` is None, returns all commits in the repository.
pub fn commits_since_tag(root: &Path, tag: Option<&str>) -> Result<Vec<Commit>> {
    let range = match tag {
        Some(t) => format!("{t}..HEAD"),
        None => "HEAD".to_string(),
    };

    let output = git_output(root, &["log", &range, "--format=%H %s"])?;
    if output.is_empty() {
        return Ok(vec![]);
    }

    let commits = output
        .lines()
        .map(|line| {
            let (hash, message) = line.split_once(' ').unwrap_or((line, ""));
            Commit {
                hash: hash.to_string(),
                message: message.to_string(),
            }
        })
        .collect();

    Ok(commits)
}

/// Return the normalized remote URL for `origin`, or None if no remote is configured.
///
/// Normalization removes the `.git` suffix and converts SSH URLs to HTTPS.
pub fn remote_url(root: &Path) -> Result<Option<String>> {
    let Some(url) = git_output_optional(root, &["remote", "get-url", "origin"])? else {
        return Ok(None);
    };
    if url.is_empty() {
        return Ok(None);
    }
    let url = url.trim_end_matches(".git");
    let url = if url.starts_with("git@") {
        // git@github.com:user/repo -> https://github.com/user/repo
        url.replacen(':', "/", 1).replacen("git@", "https://", 1)
    } else {
        url.to_string()
    };
    Ok(Some(url))
}

/// Stage the given files for commit.
pub fn stage_files(root: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["add"];
    args.extend(files);
    let success = git_success(root, &args)?;
    if !success {
        return Err(Error::Git(format!(
            "failed to stage files: {}",
            files.join(", ")
        )));
    }
    Ok(())
}

/// Create a commit with the given message.
pub fn commit(root: &Path, message: &str) -> Result<()> {
    let success = git_success(root, &["commit", "-m", message])?;
    if !success {
        return Err(Error::Git("commit failed".to_string()));
    }
    Ok(())
}

/// Create an annotated tag pointing to HEAD.
pub fn create_tag(root: &Path, tag: &str) -> Result<()> {
    let success = git_success(root, &["tag", "-a", tag, "-m", &format!("Release {tag}")])?;
    if !success {
        return Err(Error::Git(format!("failed to create tag {tag}")));
    }
    Ok(())
}

/// Push the branch and tag to origin.
pub fn push_with_tag(root: &Path, branch: &str, tag: &str) -> Result<()> {
    let success = git_success(root, &["push", "origin", branch, tag])?;
    if !success {
        return Err(Error::Git(format!("failed to push {branch} and {tag}")));
    }
    Ok(())
}
