use std::io::BufRead;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    /// The commit subject, preserving the 0.5.x library contract.
    pub message: String,
}

impl Commit {
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
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

/// Return the latest semantic-version tag other than `excluded`.
pub fn latest_semver_tag_excluding(root: &Path, excluded: &str) -> Result<Option<String>> {
    let output = git_output(root, &["tag", "--sort=-v:refname", "-l", "v*"])?;
    Ok(output
        .lines()
        .find(|tag| *tag != excluded)
        .map(str::to_string))
}

/// Check whether the given tag exists on the origin remote.
pub fn remote_tag_exists(root: &Path, tag: &str) -> Result<bool> {
    let output = git_output(
        root,
        &["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")],
    )?;
    Ok(!output.is_empty())
}

/// Return whether a named remote is configured.
pub fn remote_exists(root: &Path, remote: &str) -> Result<bool> {
    Ok(git_output_optional(root, &["remote", "get-url", remote])?.is_some())
}

/// Check whether the given tag exists in the repository.
pub fn tag_exists(root: &Path, tag: &str) -> Result<bool> {
    git_success(
        root,
        &["rev-parse", "--verify", &format!("refs/tags/{tag}")],
    )
}

/// Return whether the peeled tag target is the current commit.
pub fn tag_points_to_head(root: &Path, tag: &str) -> Result<bool> {
    let Some(tag_target) = git_output_optional(root, &["rev-parse", &format!("{tag}^{{}}")])?
    else {
        return Ok(false);
    };
    let head = git_output(root, &["rev-parse", "HEAD"])?;
    Ok(tag_target == head)
}

/// Return the annotation message for a local annotated tag.
pub fn tag_message(root: &Path, tag: &str) -> Result<Option<String>> {
    git_output_optional(root, &["tag", "--list", tag, "--format=%(contents)"])
}

/// Delete a local tag after callers have established it was not published.
pub fn delete_local_tag(root: &Path, tag: &str) -> Result<()> {
    git_output(root, &["tag", "--delete", tag])?;
    Ok(())
}

/// Return true if tracked files have staged or unstaged changes.
pub fn has_tracked_changes(root: &Path) -> Result<bool> {
    let has_staged = !git_success(root, &["diff", "--cached", "--quiet"])?;
    let has_unstaged = !git_success(root, &["diff", "--quiet"])?;
    Ok(has_staged || has_unstaged)
}

/// Return untracked, non-ignored paths relative to the repository root.
pub fn untracked_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "-z", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Git(format!("git ls-files failed: {stderr}")));
    }
    if output.stdout.is_empty() {
        return Ok(Vec::new());
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

/// Return true if tracked files have staged or unstaged changes.
///
/// This preserves the public 0.5.x behavior. Release preflight applies its
/// stricter untracked-file policy separately through [`untracked_files`].
pub fn has_uncommitted_changes(root: &Path) -> Result<bool> {
    has_tracked_changes(root)
}

/// Return the name of the currently checked-out branch.
pub fn current_branch(root: &Path) -> Result<String> {
    let branch = git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch.is_empty() {
        return Err(Error::Git("could not determine current branch".to_string()));
    }
    Ok(branch)
}

/// Return the complete message of the commit currently checked out.
pub fn head_commit_message(root: &Path) -> Result<String> {
    git_output(root, &["log", "-1", "--format=%B"])
}

/// Return whether a commit reachable from HEAD contains `marker` as a complete
/// message line. Git first returns only hashes containing the marker substring;
/// each small matching message is then checked exactly so a newer incidental
/// mention cannot hide the real prepared release ancestor.
pub fn ancestor_commit_has_marker(root: &Path, marker: &str) -> Result<bool> {
    let hashes = git_output(
        root,
        &[
            "log",
            "--format=%H",
            "--fixed-strings",
            &format!("--grep={marker}"),
        ],
    )?;
    for hash in hashes.lines() {
        let message = git_output(root, &["show", "-s", "--format=%B", hash])?;
        if message.lines().any(|line| line.trim() == marker) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return all commits reachable from HEAD since the given tag.
/// When `tag` is None, returns all commits in the repository.
pub fn commits_since_tag(root: &Path, tag: Option<&str>) -> Result<Vec<Commit>> {
    commits_since_tag_with_format(root, tag, "--format=%H%x00%s", 2)
}

/// Return compact commit metadata for changelog generation. Git extracts only
/// the subject and relevant trailers, avoiding full-history body buffering.
pub(crate) fn changelog_commits_since_tag(root: &Path, tag: Option<&str>) -> Result<Vec<Commit>> {
    let range = tag.map_or_else(|| "HEAD".to_string(), |tag| format!("{tag}..HEAD"));
    let mut child = Command::new("git")
        .args(["log", &range, "-z", "--format=%H%x00%B"])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    let stdout = child.stdout.take().expect("git stdout is piped");
    let mut reader = std::io::BufReader::new(stdout);
    let mut commits = Vec::new();

    while let Some(hash_bytes) = read_nul_field(&mut reader)? {
        if hash_bytes.is_empty() {
            continue;
        }
        let Some(message_bytes) = read_nul_field(&mut reader)? else {
            return Err(Error::Git(
                "git log returned an incomplete commit record".into(),
            ));
        };
        let hash = String::from_utf8_lossy(&hash_bytes).trim().to_string();
        // Git permits CRLF bytes in commit objects. Normalize this one bounded
        // message so footer-block detection is platform-independent without
        // buffering the full history.
        let full_message = String::from_utf8_lossy(&message_bytes).replace("\r\n", "\n");
        let mut message = full_message.lines().next().unwrap_or("").to_string();
        if let Some(value) = breaking_footer_value(&full_message) {
            message.push_str("\n\nBREAKING CHANGE: ");
            message.push_str(value);
        }
        commits.push(Commit { hash, message });
    }

    let output = child
        .wait_with_output()
        .map_err(|e| Error::Git(format!("failed to wait for git log: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Git(format!("git log failed: {stderr}")));
    }
    Ok(commits)
}

fn read_nul_field(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut field = Vec::new();
    let read = reader
        .read_until(0, &mut field)
        .map_err(|e| Error::Git(format!("read git log output: {e}")))?;
    if read == 0 {
        return Ok(None);
    }
    if field.last() == Some(&0) {
        field.pop();
    }
    Ok(Some(field))
}

fn breaking_footer_value(message: &str) -> Option<&str> {
    let (_, footer) = message.rsplit_once("\n\n")?;
    footer.lines().find_map(|line| {
        line.strip_prefix("BREAKING CHANGE:")
            .or_else(|| line.strip_prefix("BREAKING-CHANGE:"))
            .map(str::trim)
    })
}

fn commits_since_tag_with_format(
    root: &Path,
    tag: Option<&str>,
    format: &str,
    fields_per_record: usize,
) -> Result<Vec<Commit>> {
    let range = match tag {
        Some(t) => format!("{t}..HEAD"),
        None => "HEAD".to_string(),
    };

    let output = Command::new("git")
        .args(["log", &range, "-z", format])
        .current_dir(root)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Git(format!("git log failed: {stderr}")));
    }
    if output.stdout.is_empty() {
        return Ok(vec![]);
    }

    let fields: Vec<&[u8]> = output.stdout.split(|byte| *byte == 0).collect();
    let commits = fields
        .chunks_exact(fields_per_record)
        .filter_map(|record| {
            let hash = String::from_utf8_lossy(record[0]).trim().to_string();
            if hash.is_empty() {
                return None;
            }
            let message = String::from_utf8_lossy(record[1]).trim_end().to_string();
            Some(Commit { hash, message })
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

/// Return true if the index differs from HEAD, i.e. a commit would have content.
pub fn has_staged_changes(root: &Path) -> Result<bool> {
    Ok(!git_success(root, &["diff", "--cached", "--quiet"])?)
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
    let message = format!("Release {tag}\n\nVership-Release: {tag}");
    let success = git_success(root, &["tag", "-a", tag, "-m", &message])?;
    if !success {
        return Err(Error::Git(format!("failed to create tag {tag}")));
    }
    Ok(())
}

/// Push the branch and tag to origin.
pub fn push_with_tag(root: &Path, branch: &str, tag: &str) -> Result<()> {
    let branch_refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    let tag_refspec = format!("refs/tags/{tag}:refs/tags/{tag}");
    git_output(
        root,
        &["push", "--atomic", "origin", &branch_refspec, &tag_refspec],
    )?;
    Ok(())
}

#[cfg(test)]
mod changelog_stream_tests {
    use super::changelog_commits_since_tag;
    use std::process::Command;

    fn git(root: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn crlf_breaking_footer_survives_streaming_git_log() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git(root, &["init"]);
        git(root, &["config", "user.email", "test@test.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["config", "tag.gpgsign", "false"]);
        std::fs::write(root.join("file.txt"), "initial").unwrap();
        git(root, &["add", "file.txt"]);
        git(root, &["commit", "-m", "chore: initial"]);
        git(root, &["tag", "v0.1.0"]);

        std::fs::write(root.join("file.txt"), "changed").unwrap();
        git(root, &["add", "file.txt"]);
        let message_dir = tempfile::TempDir::new().unwrap();
        let message_path = message_dir.path().join("message.txt");
        std::fs::write(
            &message_path,
            b"chore: change protocol\r\n\r\nBREAKING CHANGE: wire format changed\r\n",
        )
        .unwrap();
        let output = Command::new("git")
            .args([
                "commit",
                "--cleanup=verbatim",
                "-F",
                message_path.to_str().unwrap(),
            ])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let raw = Command::new("git")
            .args(["cat-file", "commit", "HEAD"])
            .current_dir(root)
            .output()
            .unwrap()
            .stdout;
        assert!(raw.windows(4).any(|window| window == b"\r\n\r\n"));

        let commits = changelog_commits_since_tag(root, Some("v0.1.0")).unwrap();
        assert_eq!(commits.len(), 1);
        assert!(
            commits[0]
                .message
                .contains("BREAKING CHANGE: wire format changed")
        );
    }
}
