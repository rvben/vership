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
        Command::Status => vership::release::status(&output),
        Command::Preflight => vership::release::preflight(),
        Command::Changelog => vership::release::changelog_preview(),
        Command::Bump {
            level,
            dry_run,
            skip_checks,
        } => vership::release::bump(level, dry_run, skip_checks),
    }
}
