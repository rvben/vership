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
                    },
                    {
                        "name": "--yes",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip confirmation prompt (required when stdin is not a TTY)"
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
                    },
                    {
                        "name": "--yes",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip confirmation prompt (required when stdin is not a TTY)"
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
                    },
                    {
                        "name": "--yes",
                        "type": "boolean",
                        "default": false,
                        "description": "Skip confirmation prompt (required when stdin is not a TTY)"
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
                "description": "Incompatible state: the requested operation conflicts with the current repository state."
            },
            {
                "kind": "confirmation_required",
                "exit_code": 8,
                "retryable": false,
                "description": "A mutating command was invoked without a TTY and without --yes. Re-run with --yes."
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
