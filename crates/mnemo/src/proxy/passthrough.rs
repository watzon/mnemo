//! URL parsing and host validation for dynamic passthrough proxy
//!
//! This module handles the `/p/{url}` pattern for dynamic upstream routing,
//! allowing clients to specify the target LLM API URL directly in the request path.

use crate::config::ProxyConfig;
use crate::error::{MnemoError, Result};
use url::Url;

/// Represents a validated upstream target extracted from the request path
#[derive(Debug, Clone, PartialEq)]
pub struct UpstreamTarget {
    /// The parsed URL of the upstream target
    pub url: Url,
    /// The extracted host (domain or IP) from the URL
    pub host: String,
}

impl UpstreamTarget {
    /// Extract and parse a URL from the request path
    ///
    /// The path format is `/p/{url}` where `{url}` is the target upstream URL.
    /// Handles URL encoding, single-slash normalization, and various edge cases.
    ///
    /// # Arguments
    /// * `path` - The request path (e.g., "/p/https://api.openai.com/v1/chat/completions")
    /// * `query` - Optional query string to append to the URL
    ///
    /// # Returns
    /// * `Ok(UpstreamTarget)` - Successfully parsed target
    /// * `Err(MnemoError::Config)` - Invalid URL or unsupported scheme
    ///
    /// # Examples
    /// ```
    /// # use mnemo::proxy::UpstreamTarget;
    /// let target = UpstreamTarget::from_path(
    ///     "/p/https://api.openai.com/v1/chat/completions",
    ///     None
    /// ).unwrap();
    /// assert_eq!(target.host, "api.openai.com");
    /// ```
    pub fn from_path(path: &str, query: Option<&str>) -> Result<Self> {
        // Strip the leading "/p/" prefix
        let url_str = path
            .strip_prefix("/p/")
            .ok_or_else(|| MnemoError::Config(format!("Invalid passthrough path: {path}")))?;

        // URL decode the path component (Axum may pass percent-encoded sequences)
        let decoded = percent_decode(url_str)?;

        // Handle single-slash normalization (e.g., https:/example.com -> https://example.com)
        let normalized = if decoded.starts_with("http:/") && !decoded.starts_with("http://") {
            decoded.replacen("http:/", "http://", 1)
        } else if decoded.starts_with("https:/") && !decoded.starts_with("https://") {
            decoded.replacen("https:/", "https://", 1)
        } else {
            decoded.to_string()
        };

        // Parse the URL
        let mut url = Url::parse(&normalized)
            .map_err(|e| MnemoError::Config(format!("Invalid URL '{normalized}': {e}")))?;

        // Validate scheme - only HTTP and HTTPS allowed
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(MnemoError::Config(format!(
                "Unsupported URL scheme '{scheme}': only http and https are allowed"
            )));
        }

        // Extract host before any modifications
        let host = url
            .host_str()
            .ok_or_else(|| MnemoError::Config(format!("URL '{normalized}' has no host")))?
            .to_string();

        // Strip fragments (they shouldn't be sent to upstream)
        url.set_fragment(None);

        // Check for and strip userinfo (user:pass@host), logging a warning
        if url.username() != "" || url.password().is_some() {
            tracing::warn!(
                "URL contains userinfo (credentials) which has been stripped: {}",
                url
            );
            url.set_username("")
                .map_err(|_| MnemoError::Config("Failed to strip username from URL".to_string()))?;
            url.set_password(None)
                .map_err(|_| MnemoError::Config("Failed to strip password from URL".to_string()))?;
        }

        // Append query string if provided
        if let Some(q) = query {
            if !q.is_empty() {
                url.set_query(Some(q));
            }
        }

        Ok(UpstreamTarget { url, host })
    }

    /// Check if this upstream target is allowed by the proxy configuration
    ///
    /// If `config.allowed_hosts` is empty, all hosts are allowed (allow-all mode).
    /// Otherwise, the host must match one of the allowed patterns.
    ///
    /// # Supported Patterns
    /// * Exact match: "api.openai.com" matches only "api.openai.com"
    /// * Wildcard subdomain: "*.openai.com" matches "api.openai.com", "beta.openai.com", etc.
    ///
    /// # Arguments
    /// * `config` - The proxy configuration containing the allowlist
    ///
    /// # Returns
    /// * `true` - The host is allowed (or allowlist is empty)
    /// * `false` - The host is not in the allowlist
    pub fn is_allowed(&self, config: &ProxyConfig) -> bool {
        // Empty allowlist means allow all
        if config.allowed_hosts.is_empty() {
            return true;
        }

        // Check against each allowed host pattern
        for pattern in &config.allowed_hosts {
            if Self::host_matches_pattern(&self.host, pattern) {
                return true;
            }
        }

        false
    }

    /// Check if a host matches a pattern
    ///
    /// Supports exact matching and wildcard subdomain patterns.
    fn host_matches_pattern(host: &str, pattern: &str) -> bool {
        // Handle wildcard subdomain pattern: *.example.com
        if let Some(suffix) = pattern.strip_prefix("*.") {
            if host == suffix {
                // Exact match to the domain itself (e.g., "openai.com" matches "*.openai.com")
                return true;
            }
            // Check if host ends with the suffix and has at least one subdomain
            if let Some(pos) = host.rfind(suffix) {
                if pos > 0 && host[pos..] == *suffix && host[..pos].ends_with('.') {
                    return true;
                }
            }
        } else {
            // Exact match
            return host == pattern;
        }

        false
    }
}

/// Decode percent-encoded characters in a URL string
fn percent_decode(input: &str) -> Result<String> {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Try to decode percent-encoded sequence
            let hex1 = chars.next();
            let hex2 = chars.next();

            match (hex1, hex2) {
                (Some(h1), Some(h2)) => {
                    let hex_str = format!("{h1}{h2}");
                    match u8::from_str_radix(&hex_str, 16) {
                        Ok(byte) => {
                            result.push(byte as char);
                        }
                        Err(_) => {
                            // Invalid hex sequence, keep the original
                            result.push('%');
                            result.push(h1);
                            result.push(h2);
                        }
                    }
                }
                _ => {
                    // Incomplete percent encoding, keep the original
                    result.push('%');
                    if let Some(h1) = hex1 {
                        result.push(h1);
                    }
                    if let Some(h2) = hex2 {
                        result.push(h2);
                    }
                }
            }
        } else if ch == '+' {
            // Convert + to space (application/x-www-form-urlencoded format)
            result.push(' ');
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_config(allowed_hosts: Vec<String>) -> ProxyConfig {
        ProxyConfig {
            listen_addr: "127.0.0.1:9999".to_string(),
            upstream_url: None,
            allowed_hosts,
            timeout_secs: 300,
            max_injection_tokens: 2000,
        }
    }

    #[test]
    fn test_from_path_basic_https() {
        let target =
            UpstreamTarget::from_path("/p/https://api.openai.com/v1/chat/completions", None)
                .unwrap();

        assert_eq!(target.host, "api.openai.com");
        assert_eq!(
            target.url.as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_from_path_basic_http() {
        let target = UpstreamTarget::from_path("/p/http://localhost:8080/api", None).unwrap();

        assert_eq!(target.host, "localhost");
        assert_eq!(target.url.as_str(), "http://localhost:8080/api");
    }

    #[test]
    fn test_from_path_single_slash_normalization() {
        // Test https:/ -> https://
        let target = UpstreamTarget::from_path("/p/https:/api.openai.com/v1", None).unwrap();
        assert_eq!(target.url.as_str(), "https://api.openai.com/v1");

        // Test http:/ -> http://
        let target = UpstreamTarget::from_path("/p/http:/example.com/api", None).unwrap();
        assert_eq!(target.url.as_str(), "http://example.com/api");
    }

    #[test]
    fn test_from_path_with_query_string() {
        let target = UpstreamTarget::from_path(
            "/p/https://api.openai.com/v1",
            Some("model=gpt-4&temperature=0.7"),
        )
        .unwrap();

        assert_eq!(
            target.url.as_str(),
            "https://api.openai.com/v1?model=gpt-4&temperature=0.7"
        );
    }

    #[test]
    fn test_from_path_strips_fragment() {
        let target =
            UpstreamTarget::from_path("/p/https://api.openai.com/v1#section", None).unwrap();

        // Fragment should be stripped
        assert!(!target.url.as_str().contains('#'));
        assert_eq!(target.url.as_str(), "https://api.openai.com/v1");
    }

    #[test]
    fn test_from_path_strips_userinfo() {
        let target =
            UpstreamTarget::from_path("/p/https://user:pass@api.openai.com/v1", None).unwrap();

        // Userinfo should be stripped
        assert!(!target.url.as_str().contains("user:pass"));
        assert_eq!(target.url.as_str(), "https://api.openai.com/v1");
        assert_eq!(target.host, "api.openai.com");
    }

    #[test]
    fn test_from_path_rejects_non_http() {
        // Reject ftp://
        let result = UpstreamTarget::from_path("/p/ftp://ftp.example.com/file", None);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("ftp"));
        assert!(err_msg.contains("only http and https"));

        // Reject file://
        let result = UpstreamTarget::from_path("/p/file:///etc/passwd", None);
        assert!(result.is_err());

        // Reject javascript://
        let result = UpstreamTarget::from_path("/p/javascript://alert(1)", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_path_rejects_invalid_url() {
        // Missing scheme
        let result = UpstreamTarget::from_path("/p/not-a-valid-url", None);
        assert!(result.is_err());

        // Invalid path format (missing /p/ prefix)
        let result = UpstreamTarget::from_path("/invalid/path", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_allowed_empty_allowlist() {
        let target = UpstreamTarget::from_path("/p/https://api.openai.com/v1", None).unwrap();
        let config = create_config(vec![]);

        // Empty allowlist = allow all
        assert!(target.is_allowed(&config));
    }

    #[test]
    fn test_is_allowed_exact_match() {
        let target = UpstreamTarget::from_path("/p/https://api.openai.com/v1", None).unwrap();
        let config = create_config(vec!["api.openai.com".to_string()]);

        assert!(target.is_allowed(&config));

        // Different host should not match
        let target2 = UpstreamTarget::from_path("/p/https://api.anthropic.com/v1", None).unwrap();
        assert!(!target2.is_allowed(&config));
    }

    #[test]
    fn test_is_allowed_wildcard_match() {
        let config = create_config(vec!["*.openai.com".to_string()]);

        // Subdomain should match
        let target1 = UpstreamTarget::from_path("/p/https://api.openai.com/v1", None).unwrap();
        assert!(target1.is_allowed(&config));

        let target2 = UpstreamTarget::from_path("/p/https://beta.openai.com/v1", None).unwrap();
        assert!(target2.is_allowed(&config));

        // Deep subdomain should also match
        let target3 =
            UpstreamTarget::from_path("/p/https://v1.api.openai.com/endpoint", None).unwrap();
        assert!(target3.is_allowed(&config));

        // Root domain should also match *.openai.com
        let target4 = UpstreamTarget::from_path("/p/https://openai.com/", None).unwrap();
        assert!(target4.is_allowed(&config));

        // Different domain should not match
        let target5 = UpstreamTarget::from_path("/p/https://openai.org/v1", None).unwrap();
        assert!(!target5.is_allowed(&config));
    }

    #[test]
    fn test_is_allowed_blocked_host() {
        let config = create_config(vec![
            "api.openai.com".to_string(),
            "api.anthropic.com".to_string(),
        ]);

        // Allowed hosts
        let target1 = UpstreamTarget::from_path("/p/https://api.openai.com/v1", None).unwrap();
        assert!(target1.is_allowed(&config));

        // Blocked host
        let target2 = UpstreamTarget::from_path("/p/https://evil.com/api", None).unwrap();
        assert!(!target2.is_allowed(&config));
    }

    #[test]
    fn test_host_matches_pattern_edge_cases() {
        // Test exact match
        assert!(UpstreamTarget::host_matches_pattern(
            "api.openai.com",
            "api.openai.com"
        ));
        assert!(!UpstreamTarget::host_matches_pattern(
            "api.openai.com",
            "openai.com"
        ));

        // Test wildcard
        assert!(UpstreamTarget::host_matches_pattern(
            "api.openai.com",
            "*.openai.com"
        ));
        assert!(UpstreamTarget::host_matches_pattern(
            "beta.openai.com",
            "*.openai.com"
        ));
        assert!(UpstreamTarget::host_matches_pattern(
            "openai.com",
            "*.openai.com"
        ));
        assert!(!UpstreamTarget::host_matches_pattern(
            "notopenai.com",
            "*.openai.com"
        ));
        assert!(!UpstreamTarget::host_matches_pattern(
            "api.openai.org",
            "*.openai.com"
        ));
    }

    #[test]
    fn test_from_path_with_encoded_url() {
        // Test URL-encoded path
        let target = UpstreamTarget::from_path(
            "/p/https%3A%2F%2Fapi.openai.com%2Fv1%2Fchat%2Fcompletions",
            None,
        )
        .unwrap();

        assert_eq!(target.host, "api.openai.com");
        assert_eq!(
            target.url.as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_from_path_ipv6_host() {
        // IPv6 addresses in URLs
        let target = UpstreamTarget::from_path("/p/http://[::1]:8080/api", None).unwrap();

        assert_eq!(target.host, "[::1]");
        assert_eq!(target.url.as_str(), "http://[::1]:8080/api");
    }

    #[test]
    fn test_from_path_with_port() {
        let target = UpstreamTarget::from_path("/p/https://api.openai.com:8443/v1", None).unwrap();

        assert_eq!(target.host, "api.openai.com");
        assert_eq!(target.url.port(), Some(8443));
    }
}
