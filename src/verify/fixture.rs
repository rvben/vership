//! A monorepo with registered independent projects, shared by the `verify` and
//! `update_local` tests. The remote is only ever read from git config; nothing
//! is fetched.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::context::ReleaseContext;

/// Variables through which an enclosing git process (a hook in a linked
/// worktree) would redirect these commands into the developer's repository.
const GIT_LOCAL_ENV: [&str; 16] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    for var in GIT_LOCAL_ENV {
        command.env_remove(var);
    }
    let output = command.args(args).current_dir(dir).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A repository whose root publishes `mono` (PyPI, 1.0.0), a companion SDK
/// pinned to the same version, and a Cargo workspace whose member list names
/// both a root-owned crate and the registered `cli` project. Two projects are
/// registered:
///
/// - `cli` at `packages/cli` (tag prefix `cli-v`): the `mono-cli` crate at
///   0.3.0 with its own workspace, declaring tag and crates.
/// - `py` at `packages/py` (tag prefix `py-v`): the `mono-py` distribution
///   pinned to 1.0.0, the root's own version, declaring pypi.
///
/// The root's release workflow publishes to PyPI, ghcr and a Homebrew tap, so
/// anything inferred from it would show up as a target of a named project.
pub(crate) fn monorepo() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/mono.git",
        ],
    );
    write(
        root,
        "vership.toml",
        "[projects.cli]\npath = \"packages/cli\"\ntag_prefix = \"cli-v\"\n\n\
         [projects.py]\npath = \"packages/py\"\ntag_prefix = \"py-v\"\n",
    );
    write(
        root,
        ".github/workflows/release.yml",
        "steps:\n  - run: uv publish\n  - run: docker push ghcr.io/acme/mono\n  \
         - uses: acme/homebrew-tap-action@v1\n",
    );
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"packages/cli\", \"crates/core\"]\n",
    );
    write(
        root,
        "pyproject.toml",
        "[project]\nname = \"mono\"\nversion = \"1.0.0\"\n",
    );
    write(
        root,
        "sdk/python/pyproject.toml",
        "[project]\nname = \"mono-sdk\"\nversion = \"1.0.0\"\n",
    );
    write(
        root,
        "crates/core/Cargo.toml",
        "[package]\nname = \"mono-core\"\nversion = \"1.0.0\"\n",
    );
    write(root, "crates/core/src/main.rs", "fn main() {}\n");

    write(
        root,
        "packages/cli/vership.toml",
        "[verify]\ntargets = [\"tag\", \"crates\"]\n",
    );
    write(
        root,
        "packages/cli/Cargo.toml",
        "[package]\nname = \"mono-cli\"\nversion = \"0.3.0\"\n\n[workspace]\n",
    );
    write(root, "packages/cli/src/main.rs", "fn main() {}\n");

    write(
        root,
        "packages/py/vership.toml",
        "[verify]\ntargets = [\"pypi\"]\n",
    );
    write(
        root,
        "packages/py/pyproject.toml",
        "[project]\nname = \"mono-py\"\nversion = \"1.0.0\"\n",
    );
    tmp
}

pub(crate) fn context(repo: &Path, project: Option<&str>) -> ReleaseContext {
    ReleaseContext::at(repo, project).unwrap()
}
