use crate::cli::BumpLevel;
use crate::error::Result;
use crate::output::OutputConfig;

pub fn status(_output: &OutputConfig) -> Result<()> {
    todo!("status")
}

pub fn preflight() -> Result<()> {
    todo!("preflight")
}

pub fn changelog_preview() -> Result<()> {
    todo!("changelog preview")
}

pub fn bump(_level: BumpLevel, _dry_run: bool, _skip_checks: bool) -> Result<()> {
    todo!("bump")
}
