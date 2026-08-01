use std::process;

use clap::Parser;

use vership::cli::{Cli, Command, ConfigCommand};
use vership::error::Error;
use vership::output::OutputConfig;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(clap_err) => {
            // Help and version are successful display paths, not parse
            // failures: print them as clap intends and exit without an
            // error envelope.
            if matches!(
                clap_err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = clap_err.print();
                process::exit(clap_err.exit_code());
            }
            // Print clap's human-readable message first, then the structured
            // envelope as the last line of stderr (spec requirement: the
            // envelope must be mechanically extractable from the last line).
            let message = clap_err
                .to_string()
                .lines()
                .next()
                .unwrap_or("parse error")
                .trim()
                .to_string();
            // Write clap's full output to stderr, suppressing its own exit.
            let _ = clap_err.print();
            let envelope = serde_json::json!({
                "error": {
                    "kind": "error",
                    "message": message,
                }
            });
            eprintln!("{}", serde_json::to_string(&envelope).unwrap_or_default());
            process::exit(clap_err.exit_code());
        }
    };
    let output = OutputConfig::new(cli.output, cli.json);

    if let Err(e) = run(cli, output) {
        e.emit_structured();
        process::exit(e.exit_code());
    }
}

fn run(cli: Cli, output: OutputConfig) -> Result<(), Error> {
    match cli.command {
        Command::Schema => {
            use clap::CommandFactory;
            let cmd = Cli::command();
            let schema = vership::schema::generate(&cmd);
            println!(
                "{}",
                serde_json::to_string_pretty(&schema).expect("serialize")
            );
            Ok(())
        }
        Command::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "vership", &mut std::io::stdout());
            Ok(())
        }
        Command::Config(ConfigCommand::Init) => vership::config::init(),
        Command::Config(ConfigCommand::Show) => vership::config::show(&output),
        Command::Status {
            limit,
            offset,
            fields,
        } => vership::release::status(&output, limit, offset, fields.as_deref()),
        Command::Verify {
            version,
            targets,
            skip,
        } => vership::verify::run(
            version.as_deref(),
            targets.as_deref(),
            skip.as_deref(),
            &output,
        ),
        Command::UpdateLocal {
            version,
            managers,
            skip,
            dry_run,
        } => vership::update_local::run(
            version.as_deref(),
            managers.as_deref(),
            skip.as_deref(),
            dry_run,
            &output,
        ),
        Command::Preflight => vership::release::preflight(),
        Command::Changelog => vership::release::changelog_preview(),
        Command::Bump {
            level,
            dry_run,
            skip_checks,
            no_push,
        } => vership::release::bump(level, dry_run, skip_checks, no_push),
        Command::Release {
            dry_run,
            skip_checks,
            no_push,
        } => vership::release::release_current(dry_run, skip_checks, no_push),
        Command::Resume {
            dry_run,
            skip_checks,
            no_push,
        } => vership::release::resume(dry_run, skip_checks, no_push),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use vership::cli::{BumpLevel, Cli, Command, ConfigCommand, OutputFormat};

    #[test]
    fn cli_bump_patch() {
        let cli = Cli::try_parse_from(["vership", "bump", "patch"]).unwrap();
        match cli.command {
            Command::Bump {
                level,
                dry_run,
                skip_checks,
                no_push,
            } => {
                assert!(matches!(level, BumpLevel::Patch));
                assert!(!dry_run);
                assert!(!skip_checks);
                assert!(!no_push);
            }
            _ => panic!("expected Bump"),
        }
    }

    #[test]
    fn cli_bump_major_dry_run() {
        let cli = Cli::try_parse_from(["vership", "bump", "major", "--dry-run"]).unwrap();
        match cli.command {
            Command::Bump { level, dry_run, .. } => {
                assert!(matches!(level, BumpLevel::Major));
                assert!(dry_run);
            }
            _ => panic!("expected Bump"),
        }
    }

    #[test]
    fn cli_bump_skip_checks() {
        let cli = Cli::try_parse_from(["vership", "bump", "minor", "--skip-checks"]).unwrap();
        match cli.command {
            Command::Bump { skip_checks, .. } => assert!(skip_checks),
            _ => panic!("expected Bump"),
        }
    }

    #[test]
    fn cli_release() {
        let cli = Cli::try_parse_from(["vership", "release"]).unwrap();
        match cli.command {
            Command::Release {
                dry_run,
                skip_checks,
                no_push,
            } => {
                assert!(!dry_run);
                assert!(!skip_checks);
                assert!(!no_push);
            }
            _ => panic!("expected Release"),
        }
    }

    #[test]
    fn cli_release_dry_run() {
        let cli = Cli::try_parse_from(["vership", "release", "--dry-run"]).unwrap();
        match cli.command {
            Command::Release { dry_run, .. } => assert!(dry_run),
            _ => panic!("expected Release"),
        }
    }

    #[test]
    fn cli_resume() {
        let cli = Cli::try_parse_from(["vership", "resume"]).unwrap();
        assert!(matches!(cli.command, Command::Resume { .. }));
    }

    #[test]
    fn cli_resume_no_push() {
        let cli = Cli::try_parse_from(["vership", "resume", "--no-push"]).unwrap();
        match cli.command {
            Command::Resume { no_push, .. } => assert!(no_push),
            _ => panic!("expected Resume"),
        }
    }

    #[test]
    fn cli_bump_no_longer_accepts_resume_flag() {
        let result = Cli::try_parse_from(["vership", "bump", "patch", "--resume"]);
        assert!(
            result.is_err(),
            "--resume should no longer be a flag on bump"
        );
    }

    #[test]
    fn cli_status() {
        let cli = Cli::try_parse_from(["vership", "status"]).unwrap();
        assert!(matches!(cli.command, Command::Status { .. }));
    }

    #[test]
    fn cli_status_limit() {
        let cli = Cli::try_parse_from(["vership", "status", "--limit", "5"]).unwrap();
        match cli.command {
            Command::Status { limit, .. } => assert_eq!(limit, 5),
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn cli_verify_defaults() {
        let cli = Cli::try_parse_from(["vership", "verify"]).unwrap();
        match cli.command {
            Command::Verify {
                version,
                targets,
                skip,
            } => {
                assert!(version.is_none());
                assert!(targets.is_none());
                assert!(skip.is_none());
            }
            _ => panic!("expected Verify"),
        }
    }

    #[test]
    fn cli_verify_with_version_and_filters() {
        let cli = Cli::try_parse_from([
            "vership",
            "verify",
            "1.2.3",
            "--targets",
            "crates,pypi",
            "--skip",
            "npm",
        ])
        .unwrap();
        match cli.command {
            Command::Verify {
                version,
                targets,
                skip,
            } => {
                assert_eq!(version.as_deref(), Some("1.2.3"));
                assert_eq!(targets.as_deref(), Some("crates,pypi"));
                assert_eq!(skip.as_deref(), Some("npm"));
            }
            _ => panic!("expected Verify"),
        }
    }

    #[test]
    fn cli_update_local_defaults() {
        let cli = Cli::try_parse_from(["vership", "update-local"]).unwrap();
        match cli.command {
            Command::UpdateLocal {
                version,
                managers,
                skip,
                dry_run,
            } => {
                assert!(version.is_none());
                assert!(managers.is_none());
                assert!(skip.is_none());
                assert!(!dry_run);
            }
            _ => panic!("expected UpdateLocal"),
        }
    }

    #[test]
    fn cli_update_local_with_version_and_filters() {
        let cli = Cli::try_parse_from([
            "vership",
            "update-local",
            "1.2.3",
            "--managers",
            "cargo,uv",
            "--skip",
            "brew",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Command::UpdateLocal {
                version,
                managers,
                skip,
                dry_run,
            } => {
                assert_eq!(version.as_deref(), Some("1.2.3"));
                assert_eq!(managers.as_deref(), Some("cargo,uv"));
                assert_eq!(skip.as_deref(), Some("brew"));
                assert!(dry_run);
            }
            _ => panic!("expected UpdateLocal"),
        }
    }

    #[test]
    fn cli_preflight() {
        let cli = Cli::try_parse_from(["vership", "preflight"]).unwrap();
        assert!(matches!(cli.command, Command::Preflight));
    }

    #[test]
    fn cli_changelog() {
        let cli = Cli::try_parse_from(["vership", "changelog"]).unwrap();
        assert!(matches!(cli.command, Command::Changelog));
    }

    #[test]
    fn cli_schema() {
        let cli = Cli::try_parse_from(["vership", "schema"]).unwrap();
        assert!(matches!(cli.command, Command::Schema));
    }

    #[test]
    fn cli_config_init() {
        let cli = Cli::try_parse_from(["vership", "config", "init"]).unwrap();
        assert!(matches!(cli.command, Command::Config(ConfigCommand::Init)));
    }

    #[test]
    fn cli_config_show() {
        let cli = Cli::try_parse_from(["vership", "config", "show"]).unwrap();
        assert!(matches!(cli.command, Command::Config(ConfigCommand::Show)));
    }

    #[test]
    fn cli_json_flag_alias() {
        // --json is a hidden alias for --output json; backward compatibility.
        let cli = Cli::try_parse_from(["vership", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn cli_output_json() {
        let cli = Cli::try_parse_from(["vership", "--output", "json", "status"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn cli_output_text() {
        let cli = Cli::try_parse_from(["vership", "-o", "text", "status"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Text));
    }

    #[test]
    fn cli_output_default_is_auto() {
        let cli = Cli::try_parse_from(["vership", "status"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Auto));
    }

    #[test]
    fn cli_missing_subcommand_fails() {
        let result = Cli::try_parse_from(["vership"]);
        assert!(result.is_err());
    }
}
