use owo_colors::OwoColorize;
use std::io::IsTerminal;

#[derive(Clone, Copy)]
pub struct OutputConfig {
    pub json: bool,
}

impl OutputConfig {
    pub fn new(json_flag: bool) -> Self {
        let json = json_flag || !std::io::stdout().is_terminal();
        Self { json }
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
