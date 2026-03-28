use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "vership", version, about = "Multi-target release orchestrator")]
pub struct Cli {
    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Bump version, generate changelog, tag, and push
    Bump {
        /// Version bump level
        level: BumpLevel,
        /// Preview changes without modifying anything
        #[arg(long)]
        dry_run: bool,
        /// Skip lint and test checks
        #[arg(long)]
        skip_checks: bool,
        /// Stop after tagging, do not push
        #[arg(long)]
        no_push: bool,
    },
    /// Preview changelog for unreleased commits
    Changelog,
    /// Run all pre-flight checks without releasing
    Preflight,
    /// Show current version, unreleased commits, and project type
    Status,
    /// Configuration management
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Print JSON schema for agent integration
    Schema,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Create vership.toml with detected defaults
    Init,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}
