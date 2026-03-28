# Go Project Type Design

## Goal

Add Go project type support to vership, enabling tag-based releases for Go projects without requiring a version file.

## Architecture

Go uses git tags as the canonical version source — there is no version file to write. The Go module system, `go install`, and `goreleaser` all follow this convention. vership's Go support follows the same pattern: read version from the latest git tag, create a new tag on release, skip version file writes.

## Detection

- **Trigger:** `go.mod` exists and `Cargo.toml` does not
- **Priority order:** RustMaturin > Rust > Node > Go > Python
- **Config override value:** `"go"`

A Go project with `pyproject.toml` (e.g., for docs tooling) is detected as Go, not Python. Users can override via `vership.toml` if needed.

## ProjectType Trait Implementation

| Method | Behavior |
|--------|----------|
| `name()` | `"Go"` |
| `read_version()` | Parse latest semver git tag. If no tags exist, return `0.0.0`. |
| `write_version()` | No-op (returns `Ok(())`) |
| `verify_lockfile()` | Run `go mod verify` |
| `sync_lockfile()` | Run `go mod tidy` |
| `run_lint()` | Run `go vet ./...` |
| `run_tests()` | Run `go test ./...` |
| `modified_files()` | `vec!["go.mod", "go.sum"]` (tidy can modify both) |

### read_version Details

Uses existing `git::latest_semver_tag(root)` which returns `Result<Option<String>>`:
- `Some("v1.2.3")` → parse to `semver::Version(1, 2, 3)`
- `None` → return `semver::Version(0, 0, 0)` (enables first release)

This is a departure from other project types which read from files. The trait contract allows it — `root` is provided and git operations are filesystem operations.

### write_version Details

No-op. The version lives in the git tag, which `release.rs` already creates. Calling `write_version` is harmless.

### modified_files Details

Returns `["go.mod", "go.sum"]` unconditionally. `go mod tidy` can modify both files. `git add` on unchanged files is harmless — this matches the pattern used by `RustProject` which always returns `["Cargo.toml", "Cargo.lock"]`.

## Trait Addition: `is_tag_versioned`

Add a new method to the `ProjectType` trait:

```rust
/// Whether the version source is the git tag rather than a project file.
/// When true, release uses "chore: release" instead of "chore: bump version to".
fn is_tag_versioned(&self) -> bool {
    false
}
```

Default implementation returns `false`. `GoProject` overrides to return `true`.

## release.rs Changes

In the `bump()` function, change the commit message based on `is_tag_versioned()`:

```rust
let commit_msg = if project.is_tag_versioned() {
    format!("chore: release {tag}")
} else {
    format!("chore: bump version to {tag}")
};
```

No other changes needed to `release.rs`. The existing flow works:
1. `write_version` is a no-op for Go — harmless
2. `sync_lockfile` runs `go mod tidy` — good hygiene
3. `modified_files` returns `["go.mod", "go.sum"]` — staged via `git add`
4. CHANGELOG.md is generated and staged as usual
5. Commit, tag, push as usual

## Version Helpers

No new version parsing functions needed in `version.rs`. Go reads from git tags using existing `git::latest_semver_tag()`.

## Files Changed

| File | Action | Purpose |
|------|--------|---------|
| `src/project/mod.rs` | Modify | Add `pub mod go;`, add `is_tag_versioned` to trait with default impl |
| `src/project/go.rs` | Create | GoProject implementation |
| `src/project/detect.rs` | Modify | Add Go detection + `"go"` override |
| `src/release.rs` | Modify | Conditional commit message |
| `tests/go_test.rs` | Create | GoProject tests |
| `tests/detect_test.rs` | Modify | Add Go detection tests |
| `src/config.rs` | Modify | Update config init template with `"go"` |
| `README.md` | Modify | Add Go to supported project types |

## Testing

- `go_test.rs`: read_version from git tags, read_version with no tags (returns 0.0.0), write_version is no-op, modified_files returns go.mod + go.sum, name returns "Go", is_tag_versioned returns true
- `detect_test.rs`: go.mod detected as Go, go.mod + Cargo.toml detected as Rust (not Go), go.mod + package.json detected as Node (not Go), override "go" works
- Existing tests continue to pass unchanged
