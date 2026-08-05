use serde_json::{Value, json};

/// Generate a clispec v0.2 schema document.
///
/// The document is hand-authored rather than derived from the clap command tree
/// so it can carry output_fields, mutating markers, and error kinds that clap
/// has no representation for. The `_cmd` parameter is accepted for API
/// compatibility but not used.
pub fn generate(_cmd: &clap::Command) -> Value {
    json!({
        "clispec": "0.2",
        "name": "vership",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Multi-target release orchestrator",
        "global_args": [
            {
                "name": "--output",
                "type": "string",
                "enum": ["auto", "text", "json"],
                "default": "auto",
                "description": "Output format. auto emits JSON when stdout is not a TTY."
            },
            {
                "name": "-o",
                "type": "string",
                "description": "Alias for --output."
            },
            {
                "name": "--json",
                "type": "boolean",
                "default": false,
                "description": "Alias for --output json (deprecated, prefer --output)."
            }
        ],
        "commands": [
            {
                "name": "bump",
                "description": "Bump version per level, generate changelog, tag, and push. Auto-detects an interrupted prior run and continues it.",
                "mutating": true,
                "args": [
                    {
                        "name": "level",
                        "type": "string",
                        "required": true,
                        "enum": ["patch", "minor", "major"],
                        "description": "Version bump level"
                    },
                    {
                        "name": "--dry-run",
                        "type": "boolean",
                        "default": false,
                        "description": "Preview changes without modifying anything"
                    },
                    {
                        "name": "--skip-checks",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip lint and test checks"
                    },
                    {
                        "name": "--no-push",
                        "type": "boolean",
                        "default": false,
                        "description": "Stop after tagging, do not push"
                    }
                ],
                "output_fields": [
                    {
                        "name": "changed",
                        "type": "boolean",
                        "description": "Whether this invocation made any changes"
                    }
                ]
            },
            {
                "name": "release",
                "description": "Tag and release the on-disk version as-is, without bumping. Use for initial releases or when the version was set manually.",
                "mutating": true,
                "args": [
                    {
                        "name": "--dry-run",
                        "type": "boolean",
                        "default": false,
                        "description": "Preview changes without modifying anything"
                    },
                    {
                        "name": "--skip-checks",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip lint and test checks"
                    },
                    {
                        "name": "--no-push",
                        "type": "boolean",
                        "default": false,
                        "description": "Stop after tagging, do not push"
                    }
                ],
                "output_fields": [
                    {
                        "name": "changed",
                        "type": "boolean",
                        "description": "Whether this invocation made any changes"
                    }
                ]
            },
            {
                "name": "resume",
                "description": "Resume an interrupted bump. Trusts the on-disk version as the target and finishes the commit/tag/push flow.",
                "mutating": true,
                "args": [
                    {
                        "name": "--dry-run",
                        "type": "boolean",
                        "default": false,
                        "description": "Preview changes without modifying anything"
                    },
                    {
                        "name": "--skip-checks",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip lint and test checks"
                    },
                    {
                        "name": "--no-push",
                        "type": "boolean",
                        "default": false,
                        "description": "Stop after tagging, do not push"
                    }
                ],
                "output_fields": [
                    {
                        "name": "changed",
                        "type": "boolean",
                        "description": "Whether this invocation made any changes"
                    }
                ]
            },
            {
                "name": "changelog",
                "description": "Preview changelog for unreleased commits.",
                "mutating": false,
                "args": [],
                "output_fields": []
            },
            {
                "name": "preflight",
                "description": "Run all pre-flight checks without releasing.",
                "mutating": false,
                "args": [],
                "output_fields": []
            },
            {
                "name": "status",
                "description": "Show current version, unreleased commits, and project type.",
                "mutating": false,
                "args": [
                    {
                        "name": "--limit",
                        "type": "integer",
                        "required": false,
                        "default": 0,
                        "description": "Maximum number of unreleased commits to show (0 = no limit)"
                    },
                    {
                        "name": "--offset",
                        "type": "integer",
                        "required": false,
                        "default": 0,
                        "description": "Offset into the unreleased commit list for pagination"
                    },
                    {
                        "name": "--fields",
                        "type": "string",
                        "required": false,
                        "description": "Comma-separated output fields to include"
                    }
                ],
                "output_fields": [
                    {"name": "project_type", "type": "string"},
                    {
                        "name": "name",
                        "type": "string",
                        "description": "Package identity when meaningful (e.g. an Ansible collection FQCN namespace.name); omitted otherwise"
                    },
                    {"name": "current_version", "type": "string"},
                    {"name": "latest_tag", "type": "string | null"},
                    {"name": "unreleased_commits", "type": "integer"},
                    {
                        "name": "commits",
                        "type": "array",
                        "description": "Unreleased commits (subject to --limit and --offset)"
                    },
                    {
                        "name": "truncated",
                        "type": "boolean",
                        "description": "Present and true when --limit or --offset bounded the commit list"
                    },
                    {
                        "name": "total_commits",
                        "type": "integer",
                        "description": "Total unreleased commits when truncated is true"
                    }
                ]
            },
            {
                "name": "verify",
                "description": "Verify a released version is live on all publish targets (git tag, GitHub release, crates.io, PyPI, npm, Homebrew tap, ghcr). Exit 0 when all targets pass; outcome 'unpublished' (exit 8, retryable) otherwise. Compose with tarry for waiting: tarry cmd -- vership verify.",
                "mutating": false,
                "args": [
                    {
                        "name": "version",
                        "type": "string",
                        "required": false,
                        "description": "Version to verify, with or without leading v (defaults to the on-disk version)"
                    },
                    {
                        "name": "--targets",
                        "type": "string",
                        "required": false,
                        "description": "Comma-separated subset of targets to check: tag, release, crates, pypi, npm, homebrew, ghcr"
                    },
                    {
                        "name": "--skip",
                        "type": "string",
                        "required": false,
                        "description": "Comma-separated targets to skip"
                    }
                ],
                "output_fields": [
                    {"name": "version", "type": "string", "description": "Version that was verified"},
                    {"name": "ok", "type": "boolean", "description": "True when every target has the version"},
                    {
                        "name": "targets",
                        "type": "array",
                        "description": "Per-target results: {name, ok, found, detail}"
                    }
                ]
            },
            {
                "name": "update-local",
                "description": "Update this machine's installs of the released package (cargo, uv, npm, Homebrew) to a version, then report which copy $PATH actually reaches. Each manager is checked against the index its own installer resolves against before anything is installed, so a version that is not published yet reports outcome 'unpublished' (exit 8, retryable) instead of silently reinstalling the current version. An install that runs anyway and cannot resolve the version is retried once with the manager's cache bypassed, and still reports 'unpublished' rather than a general error, so a registry that has not finished propagating never looks like a permanent failure. Exit 1 when an install fails for any other reason, or when a stale or unmanaged copy shadows the updated one on $PATH. Compose with tarry to close a release: vership bump patch && tarry cmd -- vership update-local.",
                "mutating": true,
                "args": [
                    {
                        "name": "version",
                        "type": "string",
                        "required": false,
                        "description": "Version to install, with or without leading v (defaults to the on-disk version)"
                    },
                    {
                        "name": "--managers",
                        "type": "string",
                        "required": false,
                        "description": "Comma-separated subset of package managers to update: cargo, uv, npm, brew"
                    },
                    {
                        "name": "--skip",
                        "type": "string",
                        "required": false,
                        "description": "Comma-separated package managers to skip"
                    },
                    {
                        "name": "--dry-run",
                        "type": "boolean",
                        "required": false,
                        "description": "Print the install commands without running them"
                    }
                ],
                "output_fields": [
                    {"name": "version", "type": "string", "description": "Target version"},
                    {"name": "ok", "type": "boolean", "description": "True when every managed install is at the version and $PATH reaches one of them"},
                    {"name": "changed", "type": "boolean", "description": "True when at least one install was updated"},
                    {"name": "dry_run", "type": "boolean", "description": "True when nothing was executed"},
                    {
                        "name": "installs",
                        "type": "array",
                        "description": "Per-manager results: {manager, package, before, after, action, detail, commands}; action is one of already-current, updated, planned, pending, skipped, failed"
                    },
                    {
                        "name": "binaries",
                        "type": "array",
                        "description": "Per-executable $PATH resolution: {name, path, manager, version, shadowed}; path is the copy the shell runs, shadowed lists the copies behind it. A null path means the name was looked for and not found. A null manager is an unmanaged copy, whose version is deliberately not guessed. The names scanned are the project's own declared binaries as well as any an install provides, so a stale copy is still caught when no manager holds the package."
                    },
                    {
                        "name": "considered",
                        "type": "array",
                        "description": "What each manager was asked about: {manager, packages}. Distinguishes the two causes of an empty installs list: an empty considered means this project publishes nothing that manager could hold, a non-empty one means it does and none is installed. A Cargo workspace contributes one entry per member package."
                    }
                ]
            },
            {
                "name": "config",
                "description": "Configuration management.",
                "mutating": false,
                "args": [],
                "output_fields": [],
                "subcommands": [
                    {
                        "name": "init",
                        "description": "Create vership.toml with detected defaults.",
                        "mutating": true,
                        "args": [],
                        "output_fields": []
                    },
                    {
                        "name": "show",
                        "description": "Show resolved effective configuration.",
                        "mutating": false,
                        "args": [],
                        "output_fields": []
                    }
                ]
            },
            {
                "name": "schema",
                "description": "Print JSON schema for agent integration.",
                "mutating": false,
                "args": [],
                "output_fields": []
            },
            {
                "name": "completions",
                "description": "Generate shell completions.",
                "mutating": false,
                "args": [
                    {
                        "name": "shell",
                        "type": "string",
                        "required": true,
                        "enum": ["bash", "elvish", "fish", "powershell", "zsh"],
                        "description": "Shell to generate completions for"
                    }
                ],
                "output_fields": []
            }
        ],
        "outcomes": [
            {
                "kind": "unpublished",
                "exit_code": 8,
                "retryable": true,
                "description": "One or more publish targets are missing the expected version. Publishing may still be in flight; retry or wait with tarry."
            }
        ],
        "errors": [
            {
                "kind": "config",
                "exit_code": 2,
                "retryable": false,
                "description": "Configuration file is missing or malformed."
            },
            {
                "kind": "git",
                "exit_code": 3,
                "retryable": false,
                "description": "Git operation failed."
            },
            {
                "kind": "check_failed",
                "exit_code": 4,
                "retryable": false,
                "description": "A pre-flight check (lint, tests, branch, etc.) failed."
            },
            {
                "kind": "hook_failed",
                "exit_code": 5,
                "retryable": false,
                "description": "A configured hook script exited non-zero."
            },
            {
                "kind": "version",
                "exit_code": 6,
                "retryable": false,
                "description": "Version is invalid, missing, or would not produce a valid bump."
            },
            {
                "kind": "conflict",
                "exit_code": 7,
                "retryable": false,
                "description": "The target tag already exists or the requested state conflicts with immutable repository state. Re-running will not converge."
            },
            {
                "kind": "error",
                "exit_code": 1,
                "retryable": false,
                "description": "General error."
            }
        ]
    })
}
