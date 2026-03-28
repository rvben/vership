pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const CONFIG_ERROR: i32 = 2;
    pub const GIT_ERROR: i32 = 3;
    pub const CHECK_FAILED: i32 = 4;
    pub const HOOK_FAILED: i32 = 5;
    pub const VERSION_ERROR: i32 = 6;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Pre-flight check failed: {0}")]
    CheckFailed(String),

    #[error("Hook failed: {0}")]
    HookFailed(String),

    #[error("Version error: {0}")]
    Version(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Config(_) => exit_codes::CONFIG_ERROR,
            Error::Git(_) => exit_codes::GIT_ERROR,
            Error::CheckFailed(_) => exit_codes::CHECK_FAILED,
            Error::HookFailed(_) => exit_codes::HOOK_FAILED,
            Error::Version(_) => exit_codes::VERSION_ERROR,
            Error::Io(_) | Error::Other(_) => exit_codes::GENERAL_ERROR,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
