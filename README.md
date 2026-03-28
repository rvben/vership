# vership

[![crates.io](https://img.shields.io/crates/v/vership.svg)](https://crates.io/crates/vership)
[![PyPI](https://img.shields.io/pypi/v/vership.svg)](https://pypi.org/project/vership/)
[![CI](https://github.com/rvben/vership/actions/workflows/ci.yml/badge.svg)](https://github.com/rvben/vership/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A release orchestrator that handles version bumping, changelog generation, and publishing — with zero configuration.

```
$ vership bump patch
✓ No uncommitted changes
✓ On branch main
✓ Tag v0.2.1 does not exist
✓ Cargo.lock in sync
✓ Lint passes
✓ Tests pass
→ Bumping 0.2.0 → 0.2.1
→ Updated Cargo.toml
→ Generated changelog (2 entries)
→ Committed: chore: bump version to v0.2.1
→ Tagged: v0.2.1
→ Pushed to origin
```

## Why vership?

Most release tools require config files, plugins, or CI integration before they do anything. vership works out of the box: it detects your project type, runs pre-flight checks, generates a changelog from [conventional commits](https://www.conventionalcommits.org/), bumps the version, and pushes — in one command.

| | vership | cargo-release | semantic-release | git-cliff |
|---|---|---|---|---|
| Zero config | Yes | No | No | No |
| Multi-ecosystem | Rust, Node, Python, Go* | Rust only | Node only | Any (changelog only) |
| Changelog generation | Built-in | External tool | Plugin | Yes |
| Pre-flight checks | Built-in | Partial | No | No |
| Single binary | Yes | Yes | No (Node runtime) | Yes |
| Agent-friendly (`--json`, `schema`) | Yes | No | No | No |

*\* Node, Python, Go support coming soon. Architecture is ready — contributions welcome.*

## Install

```bash
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

Preview before releasing:

```bash
vership bump patch --dry-run
```

## Commands

```
vership bump <patch|minor|major>   Bump version, generate changelog, tag, push
  --dry-run                        Preview without making changes
  --skip-checks                    Skip lint and test checks
vership changelog                  Preview changelog for unreleased commits
vership preflight                  Run all pre-flight checks
vership status                     Show version, project type, unreleased commits
vership config init                Create vership.toml with defaults
vership schema                     JSON schema for agent integration
vership completions <shell>        Generate shell completions
```

## What It Does

`vership bump patch` runs this flow:

1. **Detect** project type (Rust, Rust+Maturin, more coming)
2. **Check** clean working tree, correct branch, tag doesn't exist, lockfile in sync
3. **Check** lint and tests pass (skippable with `--skip-checks`)
4. **Bump** version in project files (Cargo.toml, pyproject.toml)
5. **Generate** changelog from conventional commits since last tag
6. **Commit**, **tag**, and **push**

Your existing CI release workflow (GitHub Actions, etc.) triggers on the tag push as usual. vership handles the local side only.

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

## Configuration

**vership works without any configuration.** Only create `vership.toml` if you need to override defaults:

```toml
[project]
branch = "main"              # Branch to release from

[hooks]
pre-bump = "make verify"     # Run before version bump
post-push = "echo done"      # Run after push (e.g. trigger Homebrew update)

[checks]
lint = true                  # Run lint checks (default: true)
tests = true                 # Run tests (default: true)

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
