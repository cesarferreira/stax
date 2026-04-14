use std::fmt;

/// Exit codes for different error types
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const CONFLICT: i32 = 2;
    pub const API_ERROR: i32 = 3;
    pub const VALIDATION: i32 = 4;
    pub const AUTH: i32 = 5;
}

/// Stax-specific error types with associated exit codes
#[derive(Debug)]
pub enum StaxError {
    /// General errors (exit code 1)
    General(anyhow::Error),
    /// Git conflict errors (exit code 2)
    Conflict(String),
    /// API/network errors (exit code 3)
    Api(anyhow::Error),
    /// Validation/user input errors (exit code 4)
    Validation(String),
    /// Authentication errors (exit code 5)
    Auth(String),
}

impl StaxError {
    /// Get the exit code for this error
    pub fn exit_code(&self) -> i32 {
        match self {
            StaxError::General(_) => exit_codes::GENERAL,
            StaxError::Conflict(_) => exit_codes::CONFLICT,
            StaxError::Api(_) => exit_codes::API_ERROR,
            StaxError::Validation(_) => exit_codes::VALIDATION,
            StaxError::Auth(_) => exit_codes::AUTH,
        }
    }

    /// Create a conflict error
    pub fn conflict(msg: impl Into<String>) -> Self {
        StaxError::Conflict(msg.into())
    }

    /// Create an API error
    pub fn api(err: anyhow::Error) -> Self {
        StaxError::Api(err)
    }

    /// Create a validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        StaxError::Validation(msg.into())
    }

    /// Create an auth error
    pub fn auth(msg: impl Into<String>) -> Self {
        StaxError::Auth(msg.into())
    }
}

impl fmt::Display for StaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StaxError::General(err) => write!(f, "{}", err),
            StaxError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            StaxError::Api(err) => write!(f, "API error: {}", err),
            StaxError::Validation(msg) => write!(f, "Validation error: {}", msg),
            StaxError::Auth(msg) => write!(f, "Authentication error: {}", msg),
        }
    }
}

impl std::error::Error for StaxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StaxError::General(err) | StaxError::Api(err) => err.source(),
            _ => None,
        }
    }
}

// Allow converting from anyhow::Error to StaxError::General
impl From<anyhow::Error> for StaxError {
    fn from(err: anyhow::Error) -> Self {
        StaxError::General(err)
    }
}

/// Result type using StaxError
pub type StaxResult<T> = Result<T, StaxError>;

/// Sentinel error returned when a rebase stops on conflict.
///
/// This is not a "real" error — the conflict information has already been
/// printed. The sentinel propagates through `anyhow::Result` so that
/// `cli::run()` can intercept it and exit with code 1 without printing
/// an additional `Error: …` line.
#[derive(Debug)]
pub struct ConflictStopped;

impl std::fmt::Display for ConflictStopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stopped on rebase conflict")
    }
}

impl std::error::Error for ConflictStopped {}
