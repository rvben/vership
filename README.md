# vership

[![crates.io](https://img.shields.io/crates/v/vership.svg)](https://crates.io/crates/vership)
[![PyPI](https://img.shields.io/pypi/v/vership.svg)](https://pypi.org/project/vership/)
[![CI](https://github.com/rvben/vership/actions/workflows/ci.yml/badge.svg)](https://github.com/rvben/vership/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![codecov](https://codecov.io/gh/rvben/vership/graph/badge.svg)](https://codecov.io/gh/rvben/vership)

A release orchestrator that handles version bumping, changelog generation, and publishing — with zero configuration.

```
$ vership bump patch
✓ No uncommitted changes
✓ On branch main
✓ Tag v0.4.1 does not exist
✓ Lock file in sync
✓ Lint passes
✓ Tests pass
→ Bumping 0.4.0 → 0.4.1
→ Updated rust
→ Updating version files
→ Generated changelog (3 entries)
→ Running artifact: cargo run --release -- schema generate
→ Committed: chore: bump version to v0.4.1
→ Tagged: v0.4.1
→ Pushed to origin
```

## Why vership?

Most release tools require config files, plugins, or CI integration before they do anything. vership works out of the box: it detects your project type, runs pre-flight checks, generates a changelog from [conventional commits](https://www.conventionalcommits.org/), bumps the version, and pushes — in one command.

| | vership | cargo-release | semantic-release | git-cliff |
|---|---|---|---|---|
| Zero config | Yes | No | No | No |
| Multi-ecosystem | Rust, Node, Python, Go, Gradle, Ansible | Rust only | Node only | Any (changelog only) |
| Changelog generation | Built-in | External tool | Plugin | Yes |
| Multi-file version sync | Built-in | No | Plugin | No |
| Artifact regeneration | Built-in | No | Plugin | No |
| Pre-flight checks | Built-in | Partial | No | No |
| Single binary | Yes | Yes | No (Node runtime) | Yes |
| Agent-friendly (`--json`, `schema`) | Yes | No | No | No |


## Install

```bash
# Homebrew
brew install rvben/tap/vership

# From crates.io
cargo install vership

# From PyPI
pip install vership

# From source
git clone https://github.com/rvben/vership && cd vership && cargo install --path .
```

## Quick Start

No setup required. Just use conventional commits and run:

```bash
vership bump patch    # 0.1.0 → 0.1.1
vership bump minor    # 0.1.1 → 0.2.0
vership bump major    # 0.2.0 → 1.0.0
```

Initial release? Tag the version already in your manifest:

```bash
vership release       # tag the current Cargo.toml/package.json version as-is
```

Interrupted run? Continue where it stopped:

```bash
vership resume        # finishes commit/tag/push using the on-disk version
```

Preview before releasing:

```bash
vership bump patch --dry-run
```

## Commands

```
vership bump <patch|minor|major>   Bump version, generate changelog, tag, push
  --dry-run                        Preview without making changes
  --skip-checks                    Skip lint and test checks
  --no-push                        Stop after tagging, do not push
vership release                    Tag the on-disk version as-is (no bump)
  --dry-run / --skip-checks / --no-push    same as bump
vership resume                     Finish an interrupted bump (trusts on-disk version)
  --dry-run / --skip-checks / --no-push    same as bump
vership changelog                  Preview changelog for unreleased commits
vership preflight                  Run all pre-flight checks
vership status                     Show version, project type, unreleased commits
vership verify [<version>]         Verify a release is live on all publish targets
  --targets <list> / --skip <list>         Filter targets
vership update-local [<version>]   Update this machine's installs to a version
  --managers <list> / --skip <list>        Filter package managers
  --dry-run                        Print the install commands without running them
vership config init                Create vership.toml with defaults
vership schema                     JSON schema for agent integration
vership completions <shell>        Generate shell completions
```

`bump` auto-detects an interrupted prior run when the on-disk version already
matches the expected post-bump value AND the working tree is dirty. The
explicit `resume` subcommand is the escape hatch for cases where auto-detection
doesn't fire.

## What It Does

`vership bump patch` runs this flow:

1. **Detect** project type (Rust, Rust+Maturin, Node, Go, Python, Gradle, Ansible Collection)
2. **Check** clean working tree, correct branch, tag doesn't exist, lockfile in sync
3. **Check** lint and tests pass (skippable with `--skip-checks`)
4. **Bump** version in project files (Cargo.toml, package.json, pyproject.toml, gradle.properties, galaxy.yml) or tag directly (Go)
5. **Update** version references in extra files (`version_files`)
6. **Generate** changelog from conventional commits since last tag
7. **Regenerate** artifacts from commands (`artifacts`)
8. **Commit**, **tag**, and **push**

A repo carrying several manifests is resolved by precedence: `galaxy.yml`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, Gradle. A `package.json` marked `"private": true` is skipped in that order and considered only once nothing else matches, so a vendored test harness or docs site does not outrank the manifest the repo actually releases. Set `[project] type` in vership.toml to override detection entirely.

Your existing CI release workflow (GitHub Actions, etc.) triggers on the tag push as usual. vership handles the local side only.

## Post-Release Verification

A pushed tag does not mean the release published: a CI job can fail after tagging, skip a registry, or upload an empty release. `vership verify` checks that a version is actually live everywhere the repo publishes:

```
$ vership verify
verify 0.5.6
  ok   tag        v0.5.6
  ok   release    0.5.6
  ok   crates     0.5.6
  FAIL pypi       not found
```

Targets are autodetected: `tag` and `release` from the GitHub remote, `crates` from Cargo.toml (unless `publish = false`), `pypi` from pyproject.toml, `npm` from a non-private package.json, `homebrew` and `ghcr` from publish steps in `.github/workflows/`. The `[verify]` section in vership.toml can skip targets or set the tap, formula, and image coordinates:

```toml
[verify]
skip = ["npm"]
tap = "owner/homebrew-tap"     # default: <owner>/homebrew-tap
formula = "name"               # default: repo name
image = "owner/name"           # default: owner/repo lowercased
```

Exit codes: 0 when every target passes; 8 (`unpublished`, retryable) when anything is missing, version-mismatched, or errored. Publishing may still be in flight, so 8 is a state, not a failure. To block until a release is fully live, compose with [tarry](https://github.com/rvben/tarry):

```sh
tarry cmd --timeout 20m -- vership verify
```

## Local Install Update

Releasing a tool you use yourself leaves your own machine on the old version, and often on several old versions at once: the same executable can be installed by cargo, uv, npm and Homebrew, and whichever directory comes first on `$PATH` is the one you actually run. `vership update-local` closes that gap:

```
$ vership update-local
update-local 0.5.13
  ok   cargo      0.5.12 -> 0.5.13
  ok   uv         0.5.13
  ok   vership    /Users/you/.cargo/bin/vership (cargo 0.5.13)
                  shadowed /usr/local/bin/vership (unmanaged)
```

Only managers that already hold the package are touched; nothing is newly installed. The package name each manager is asked about comes from the same detection `verify` uses: the crate name from Cargo.toml, the project name from pyproject.toml, the package name from package.json, the formula from the detected tap.

Before anything is installed, each manager's registry is checked for the target version. This matters because an unpinned reinstall is a silent downgrade: `cargo install <crate> --force` fetches whatever the registry currently serves and exits 0, so running it a minute after `vership bump` would reinstall the version you just replaced. A registry that has not caught up reports `unpublished` (exit 8, retryable) and runs nothing, for every manager and not only the lagging one: registries publish at different speeds on the same release, and a half-updated machine is not a state "retry and you are done" can describe. Exit 8 therefore means the machine is exactly as it was. `--dry-run` reports the same wait and the same exit 8, because a preview that claimed the installs would run would be predicting something a real run at that moment would not do; the commands are still printed. It composes with [tarry](https://github.com/rvben/tarry) to close a release in one line:

```sh
vership bump patch && tarry cmd --timeout 20m -- vership update-local
```

After the installs, every copy of every executable on `$PATH` is reported in `$PATH` order. Ownership is decided by resolved file identity, not by directory, so a hand-copied binary sitting next to a uv shim is reported as unmanaged rather than credited to uv, and its version is left blank rather than guessed. A copy resolving into a Homebrew keg is the one exception, credited to brew at the version the keg path states, since Homebrew owns everything under `Cellar/` by construction. That covers the copies no manager was asked about: brew is probed only when the project's tap is detected. Exit 1 when an install fails, or when a stale or unmanaged copy shadows the one that was updated: an update nothing reaches is not an update.

Installs that would not reproduce the release are reported and left alone: a git or alternate-registry install, and a `cargo install --path` install of a different project. A `--path` install of the project you are in is rebuilt from it.

Caveat: the uv update is a pinned `uv tool install <pkg>==<version> --reinstall --refresh`, which is the only deterministic form (`uv tool upgrade` has no `--refresh` and reports nothing to do against a cached index). A pinned reinstall drops `--with` extras the tool was originally installed with; re-add them by hand if you use them.

## Changelog Format

Generated from [conventional commits](https://www.conventionalcommits.org/) in [Keep a Changelog](https://keepachangelog.com/) format:

```markdown
## [0.2.1](https://github.com/you/repo/compare/v0.2.0...v0.2.1) - 2026-03-28

### Added

- **api**: add user endpoint ([abc1234](https://github.com/you/repo/commit/abc1234))

### Fixed

- correct null handling in parser ([def5678](https://github.com/you/repo/commit/def5678))
```

| Commit type | Section |
|-------------|---------|
| `feat` | Added |
| `fix` | Fixed |
| `perf` | Performance |
| `change` | Changed |
| `feat!` / `BREAKING CHANGE` | Breaking Changes |
| `chore`, `docs`, `ci`, `test`, `refactor`, `build`, `style` | Excluded |

## Version Files

Projects often have version strings scattered across READMEs, docs, and companion packages. vership updates them all during the bump:

```toml
# Text mode: search/replace with placeholders
[[version_files]]
glob = "README.md"
search = "rev: v{prev}"       # {prev} = old version
replace = "rev: v{version}"   # {version} = new version

# Field mode: update JSON fields directly
[[version_files]]
glob = "npm/*/package.json"
field = "version"

# Wildcard: update all values in an object
[[version_files]]
glob = "package.json"
field = "optionalDependencies.*"
```

All matched files are staged and included in the release commit automatically.

## Artifacts

Some projects need to regenerate files from the built binary during release (schemas, rule exports, API docs). vership runs these commands and commits the output:

```toml
# Capture stdout to a file
[[artifacts]]
command = "cargo run --release -- schema generate-json"
output = "schema.json"

# Or let the command write its own files
[[artifacts]]
command = "make generate-docs"
files = ["docs/api.json"]
```

Commands run from the project root via `sh -c`. Output files are staged automatically. If a declared file doesn't exist after the command runs, the release aborts with a clear error.

## Configuration

**vership works without any configuration.** Only create `vership.toml` if you need to override defaults:

```toml
[project]
branch = "main"              # Branch to release from
type = "go"                  # Skip detection: rust, rust-maturin, node, go,
                             # python, gradle, ansible-collection

[hooks]
pre-bump = "make verify"     # Run before version bump
post-push = "echo done"      # Run after push (e.g. trigger Homebrew update)

[checks]
lint = true                  # Run lint checks (default: true)
tests = true                 # Run tests (default: true)
lint_command = "npm run lint" # Override default lint command
test_command = "npm test"     # Override default test command

[changelog]
unconventional = "exclude"   # "exclude", "include", or "strict"
```

## Agent Integration

vership is designed to work with AI coding assistants:

```bash
# Machine-readable project status
vership status --json

# Full command schema for tool discovery
vership schema
```

## License

MIT

## Releasing

Vership owns versioning, changelog generation, release commits, and tags. See
[the release runbook](docs/releases.md) for the verified workflow and recovery policy.
