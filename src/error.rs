pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const CONFIG_ERROR: i32 = 2;
    pub const GIT_ERROR: i32 = 3;
    pub const CHECK_FAILED: i32 = 4;
    pub const HOOK_FAILED: i32 = 5;
    pub const VERSION_ERROR: i32 = 6;
    pub const CONFLICT: i32 = 7;
    pub const UNPUBLISHED: i32 = 8;
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

    /// The requested operation cannot converge: the target state already exists
    /// or conflicts with immutable repository state. Re-running will not help.
    #[error("Conflict: {0}")]
    Conflict(String),

    /// One or more publish targets are missing the expected version.
    /// Retryable: publishing may still be in flight.
    #[error("Unpublished: {0}")]
    Unpublished(String),

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
            Error::Conflict(_) => exit_codes::CONFLICT,
            Error::Unpublished(_) => exit_codes::UNPUBLISHED,
            Error::Io(_) | Error::Other(_) => exit_codes::GENERAL_ERROR,
        }
    }

    /// Stable kind string used in structured error output and the schema.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Config(_) => "config",
            Error::Git(_) => "git",
            Error::CheckFailed(_) => "check_failed",
            Error::HookFailed(_) => "hook_failed",
            Error::Version(_) => "version",
            Error::Conflict(_) => "conflict",
            Error::Unpublished(_) => "unpublished",
            Error::Io(_) | Error::Other(_) => "error",
        }
    }

    /// Write the structured error envelope as the last line of stderr.
    ///
    /// Format: `{"error":{"kind":"...","message":"..."}}`.
    pub fn emit_structured(&self) {
        let envelope = serde_json::json!({
            "error": {
                "kind": self.kind(),
                "message": self.to_string(),
            }
        });
        eprintln!("{}", serde_json::to_string(&envelope).unwrap_or_default());
    }
}

pub type Result<T> = std::result::Result<T, Error>;
