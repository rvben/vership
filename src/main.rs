use std::process;

use clap::Parser;

use vership::cli::{Cli, Command, ConfigCommand};
use vership::error::Error;
use vership::output::OutputConfig;

fn main() {
    let cli = Cli::parse();
    let output = OutputConfig::new(cli.json);

    if let Err(e) = run(cli, output) {
        eprintln!("Error: {e}");
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
        Command::Config(ConfigCommand::Show) => vership::config::show(output.json),
        Command::Status => vership::release::status(&output),
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
    use vership::cli::{BumpLevel, Cli, Command, ConfigCommand};

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
        assert!(matches!(cli.command, Command::Status));
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
    fn cli_json_flag() {
        let cli = Cli::try_parse_from(["vership", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn cli_missing_subcommand_fails() {
        let result = Cli::try_parse_from(["vership"]);
        assert!(result.is_err());
    }
}
