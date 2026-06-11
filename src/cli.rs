use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

/// Output format. Default `auto` selects JSON when stdout is not a TTY.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JSON when piped, human-readable on a TTY.
    #[default]
    Auto,
    /// Always human-readable text.
    Text,
    /// Always JSON.
    Json,
}

#[derive(Parser)]
#[command(name = "vership", version, about = "Multi-target release orchestrator")]
pub struct Cli {
    /// Output format: auto (default), text, or json.
    /// `auto` emits JSON when stdout is not a TTY, human-readable otherwise.
    #[arg(
        long = "output",
        short = 'o',
        global = true,
        value_name = "FORMAT",
        default_value = "auto"
    )]
    pub output: OutputFormat,

    /// Output as JSON (alias for --output json).
    #[arg(long, global = true, hide = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Bump version per `level`, generate changelog, tag, and push.
    /// Auto-detects an interrupted prior run and continues it.
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
    /// Tag and release the on-disk version as-is, without bumping.
    /// Use for initial releases or when the version was set manually.
    Release {
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
    /// Resume an interrupted bump. Trusts the on-disk version as the target
    /// and finishes the commit/tag/push flow.
    Resume {
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
    Status {
        /// Maximum number of unreleased commits to show (0 = no limit)
        #[arg(long, default_value = "0")]
        limit: usize,
        /// Offset into the unreleased commit list (for pagination)
        #[arg(long, default_value = "0")]
        offset: usize,
        /// Comma-separated output fields to include (e.g. project_type,current_version)
        #[arg(long, value_name = "FIELDS")]
        fields: Option<String>,
    },
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
    /// Show resolved effective configuration
    Show,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}
