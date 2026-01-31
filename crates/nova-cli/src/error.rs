use std::fmt;

#[derive(Debug)]
pub struct CliError(pub String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CliError {}

impl From<String> for CliError {
    fn from(s: String) -> Self {
        CliError(s)
    }
}

impl From<&str> for CliError {
    fn from(s: &str) -> Self {
        CliError(s.to_string())
    }
}

impl From<nova_memory::NovaError> for CliError {
    fn from(e: nova_memory::NovaError) -> Self {
        CliError(e.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self {
        CliError(format!("JSON error: {}", e))
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError(format!("IO error: {}", e))
    }
}

pub type CliResult<T> = Result<T, CliError>;
