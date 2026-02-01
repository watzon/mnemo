//! Session ID validation for proxy layer
//!
//! Session IDs are used to track and correlate requests across the proxy.
//! They must be alphanumeric with underscores and hyphens, max 128 chars.

use thiserror::Error;

/// Maximum length for session IDs
const MAX_SESSION_ID_LEN: usize = 128;

/// Errors that can occur during session ID validation
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SessionIdError {
    /// Session ID is empty
    #[error("Session ID cannot be empty")]
    Empty,

    /// Session ID contains invalid characters
    #[error("Session ID contains invalid characters: allowed are a-z, A-Z, 0-9, _, -")]
    InvalidChars,

    /// Session ID exceeds maximum length
    #[error("Session ID exceeds maximum length of {MAX_SESSION_ID_LEN} characters")]
    TooLong,
}

/// A validated session ID
///
/// Session IDs must:
/// - Be non-empty
/// - Contain only alphanumeric characters, underscores, and hyphens
/// - Be at most 128 characters long
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Get the session ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate a string as a session ID
    fn validate(s: &str) -> Result<(), SessionIdError> {
        if s.is_empty() {
            return Err(SessionIdError::Empty);
        }

        if s.len() > MAX_SESSION_ID_LEN {
            return Err(SessionIdError::TooLong);
        }

        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(SessionIdError::InvalidChars);
        }

        Ok(())
    }
}

impl TryFrom<&str> for SessionId {
    type Error = SessionIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::validate(value)?;
        Ok(SessionId(value.to_string()))
    }
}

impl TryFrom<String> for SessionId {
    type Error = SessionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(SessionId(value))
    }
}

impl From<SessionId> for String {
    fn from(session_id: SessionId) -> Self {
        session_id.0
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_session_ids() {
        // Standard valid cases
        assert!(SessionId::try_from("project-abc").is_ok());
        assert!(SessionId::try_from("PROJECT_123").is_ok());
        assert!(SessionId::try_from("a-b_c").is_ok());
        assert!(SessionId::try_from("a").is_ok()); // Single char
        assert!(SessionId::try_from("123").is_ok()); // Numbers only
        assert!(SessionId::try_from("abc_def-ghi").is_ok()); // Mixed
        assert!(SessionId::try_from("ABC-DEF_GHI").is_ok()); // Uppercase
    }

    #[test]
    fn test_empty_session_id() {
        let result = SessionId::try_from("");
        assert!(matches!(result, Err(SessionIdError::Empty)));
    }

    #[test]
    fn test_whitespace_only_session_id() {
        // Whitespace is not allowed (invalid chars)
        let result = SessionId::try_from("   ");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_with_spaces() {
        let result = SessionId::try_from("has spaces");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_with_special_chars() {
        let result = SessionId::try_from("has!special");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test@email");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test#hash");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_too_long() {
        // Create a 129 character string
        let long_id = "a".repeat(129);
        let result = SessionId::try_from(long_id.as_str());
        assert!(matches!(result, Err(SessionIdError::TooLong)));
    }

    #[test]
    fn test_session_id_at_max_length() {
        // 128 characters should be valid
        let max_id = "a".repeat(128);
        assert!(SessionId::try_from(max_id.as_str()).is_ok());
    }

    #[test]
    fn test_try_from_string() {
        // Test TryFrom<String>
        let s = String::from("valid-id");
        assert!(SessionId::try_from(s).is_ok());

        let s = String::from("has spaces");
        assert!(matches!(
            SessionId::try_from(s),
            Err(SessionIdError::InvalidChars)
        ));
    }

    #[test]
    fn test_into_string() {
        let session_id = SessionId::try_from("test-id").unwrap();
        let s: String = session_id.into();
        assert_eq!(s, "test-id");
    }

    #[test]
    fn test_as_str() {
        let session_id = SessionId::try_from("test-id").unwrap();
        assert_eq!(session_id.as_str(), "test-id");
    }

    #[test]
    fn test_as_ref() {
        let session_id = SessionId::try_from("test-id").unwrap();
        assert_eq!(session_id.as_ref(), "test-id");
    }

    #[test]
    fn test_display() {
        let session_id = SessionId::try_from("test-id").unwrap();
        assert_eq!(format!("{}", session_id), "test-id");
    }

    #[test]
    fn test_clone() {
        let session_id = SessionId::try_from("test-id").unwrap();
        let cloned = session_id.clone();
        assert_eq!(session_id, cloned);
    }

    #[test]
    fn test_case_sensitive() {
        // Session IDs are case-sensitive
        let lower = SessionId::try_from("abc").unwrap();
        let upper = SessionId::try_from("ABC").unwrap();
        assert_ne!(lower, upper);
        assert_eq!(lower.as_str(), "abc");
        assert_eq!(upper.as_str(), "ABC");
    }
}
