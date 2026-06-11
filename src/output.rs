use owo_colors::OwoColorize;
use std::io::IsTerminal;

use crate::cli::OutputFormat;

#[derive(Clone, Copy)]
pub struct OutputConfig {
    format: OutputFormat,
}

impl OutputConfig {
    /// Resolve `auto` by TTY detection. Explicit `text` or `json` always wins.
    /// `json_alias` is true when the legacy `--json` flag was passed.
    pub fn new(format: OutputFormat, json_alias: bool) -> Self {
        let resolved = if json_alias {
            OutputFormat::Json
        } else {
            format
        };
        Self { format: resolved }
    }

    /// Returns true when structured JSON should be emitted to stdout.
    pub fn is_json(&self) -> bool {
        match self.format {
            OutputFormat::Json => true,
            OutputFormat::Text => false,
            OutputFormat::Auto => !std::io::stdout().is_terminal(),
        }
    }
}

pub fn use_color() -> bool {
    std::io::stdout().is_terminal()
}

pub fn print_check_pass(msg: &str) {
    if use_color() {
        eprintln!("{} {}", "✓".green(), msg);
    } else {
        eprintln!("OK {}", msg);
    }
}

pub fn print_check_fail(msg: &str) {
    if use_color() {
        eprintln!("{} {}", "✗".red(), msg);
    } else {
        eprintln!("FAIL {}", msg);
    }
}

pub fn print_step(msg: &str) {
    if use_color() {
        eprintln!("{} {}", "→".cyan(), msg);
    } else {
        eprintln!("  {}", msg);
    }
}
