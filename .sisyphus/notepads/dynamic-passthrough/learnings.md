# Dynamic Passthrough Learnings

## 2026-02-01 Session Start

### Workspace Structure
- Root `Cargo.toml` uses `[workspace.dependencies]` pattern
- Crate dependencies use `{ workspace = true }` to reference workspace deps
- Edition 2024, MSRV 1.85 (bleeding edge Rust)

### Current ProxyConfig Structure (config/mod.rs:67-92)
```rust
pub struct ProxyConfig {
    pub listen_addr: String,       // with default "127.0.0.1:9999"
    pub upstream_url: String,      // NO default - required field
    pub timeout_secs: u64,         // default 300
    pub max_injection_tokens: usize, // default 2000
}
```

### Testing Notes
- `--test-threads=1` required for ML model tests (contention)
- Tests use `tempfile::tempdir()` for isolated LanceDB instances

## Task 2: ProxyConfig Schema Changes (2026-01-31)

### Changes Made
- Modified `ProxyConfig` struct in `crates/nova-memory/src/config/mod.rs`
- Changed `upstream_url` from `String` to `Option<String>` with `#[serde(default)]`
- Added `allowed_hosts: Vec<String>` field with `#[serde(default)]`
- Updated Default impl to set `upstream_url: None` and `allowed_hosts: Vec::new()`
- Updated all existing tests to handle `Option<String>`
- Added 3 new tests:
  - `test_upstream_url_none_when_not_provided`
  - `test_allowed_hosts_defaults_to_empty`
  - `test_allowed_hosts_parses_from_toml`

### Pattern: Option<T> with serde(default)
When making a field optional in a config struct:
1. Change type from `T` to `Option<T>`
2. Add `#[serde(default)]` attribute (not `default = "..."` function)
3. In Default impl, set to `None`
4. In tests, use `Some("value".to_string())` for comparisons

### Test Update Pattern
When updating existing tests for Option<String>:
- Change: `assert_eq!(config.field, "value")`
- To: `assert_eq!(config.field, Some("value".to_string()))`
- For None: `assert!(config.field.is_none())`

### Cross-File Dependencies
Found that `cli_tests.rs` also had assertions on `upstream_url` that needed updating. Always search for field usage across the codebase when making schema changes.

### Pre-existing Issues
Discovered missing `urlencoding` dependency in Cargo.toml that was blocking compilation. Added it as a temporary fix to enable testing.

### Verification Command
```bash
cargo test -p nova-memory config -- --test-threads=1
```
All 26 config-related tests passed.

## Task 1: Passthrough Module Implementation (2026-01-31)

### Implementation Summary
Created `crates/nova-memory/src/proxy/passthrough.rs` with URL parsing and host validation for the `/p/{url}` passthrough pattern.

### Key Components
- `UpstreamTarget` struct with `url: Url` and `host: String` fields
- `from_path()` method for URL extraction from `/p/{url}` paths
- `is_allowed()` method for allowlist checking against `ProxyConfig.allowed_hosts`

### URL Parsing Features
- Strips `/p/` prefix from path
- Handles single-slash normalization (`https:/` → `https://`)
- Percent-decodes URL-encoded characters
- Strips fragments (`#section`)
- Strips userinfo (`user:pass@`) with warning log
- Rejects non-HTTP(S) schemes (ftp, file, javascript, etc.)
- Appends query string if provided
- Returns `NovaError::Config` for all validation errors

### Host Allowlist Patterns
- Empty allowlist = allow all (permissive default)
- Exact match: `"api.openai.com"` matches only that host
- Wildcard subdomain: `"*.openai.com"` matches any subdomain + root domain

### Testing
Added 17 test cases covering:
- Basic HTTPS/HTTP URL parsing
- Single-slash normalization
- Query string handling
- Fragment stripping
- Userinfo stripping
- Non-HTTP scheme rejection
- Invalid URL handling
- Empty allowlist (allow all)
- Exact host matching
- Wildcard subdomain matching
- Blocked host rejection
- URL-encoded paths
- IPv6 hosts
- Port handling

### Dependencies
- Used existing `url` crate (already in workspace dependencies)
- Implemented custom `percent_decode()` function (no external crate needed)
- Used `tracing` for warning logs

### Module Integration
- Added `mod passthrough;` to `proxy/mod.rs`
- Added `pub use passthrough::UpstreamTarget;` for public export

### Verification
```bash
cargo test -p nova-memory passthrough -- --test-threads=1
```
All 17 passthrough tests passed.

### Pattern: Custom Percent-Decoding
When `urlencoding` crate is not available, implement simple percent-decoding:
```rust
fn percent_decode(input: &str) -> Result<String> {
    // Iterate chars, decode %XX sequences using u8::from_str_radix
    // Handle + as space for form-urlencoded compatibility
}
```

### Pattern: URL Validation Pipeline
1. Strip prefix
2. Percent-decode
3. Normalize slashes
4. Parse with `Url::parse()`
5. Validate scheme
6. Extract host
7. Strip fragments/userinfo
8. Append query string

## Task 5: Config Example Documentation (2026-01-31)

### Changes Made
- Updated `config.example.toml` `[proxy]` section
- Commented out `upstream_url` with explanation about optional usage with dynamic passthrough
- Added `allowed_hosts` field with comprehensive security documentation
- Added usage examples showing `/p/{url}` route patterns

### Config Documentation Pattern
Configuration example files require extensive inline comments:
1. **Header comment** explaining field purpose and behavior
2. **Behavior variations** as bullet points (empty = allow all, wildcards, etc.)
3. **Security warnings** in prominent locations (all caps, clear language)
4. **Concrete examples** showing actual usage patterns
5. **Field placement**: Commented if optional, uncommented with default if required

### TOML Validation
When taplo is not available, Python's `tomllib` (3.11+) works as a reliable fallback:
```bash
python3 -c "import tomllib; tomllib.load(open('config.example.toml', 'rb')); print('Valid')"
```

### Security Documentation Best Practices
- Use explicit WARNING labels for security implications
- Explain what happens with empty/default values
- Provide concrete examples of both safe and unsafe configurations
- Document wildcard patterns clearly with examples

## Task 2: Server.rs Rewrite for Dynamic Passthrough

### Implementation Details
- Used `{*upstream_url}` wildcard pattern for Axum routing (not `*upstream_url`)
- `UpstreamTarget::from_path` expects full path with `/p/` prefix
- Axum 0.8 uses `routing::any()` for all HTTP methods
- Must convert between `axum::http::HeaderMap` and `reqwest::header::HeaderMap`
- Method conversion requires explicit mapping for common methods

### Hop-by-hop Headers
Headers stripped during proxying: `host`, `connection`, `keep-alive`, `transfer-encoding`, `proxy-connection`, `te`, `upgrade`

### Error Handling Pattern
- Invalid URL → 400 Bad Request with `invalid_url` error type
- Host blocked → 403 Forbidden with `host_not_allowed` error type
- No upstream configured → 404 Not Found with `no_upstream_configured`
- Network errors → ProxyError::Network for timeout/connection failures

### Test Coverage
5 unit tests covering:
- Health check endpoint
- Fallback without upstream URL
- Invalid passthrough URL
- Blocked host rejection
- Hop-by-hop header constants

## Task 6: Passthrough Integration Tests (2026-01-31)

### Changes Made
- Made `AppState` derive `Clone` in `proxy/server.rs`
- Made `create_router` public in `proxy/server.rs`
- Exported `AppState` and `create_router` from `proxy/mod.rs`
- Added 16 new passthrough tests to `proxy_tests.rs`:
  - `test_passthrough_basic_post` - POST to mock returns response
  - `test_passthrough_get_with_query_string` - Query params forwarded
  - `test_passthrough_headers_forwarded` - Authorization header passes through
  - `test_passthrough_hop_by_hop_stripped` - Connection header not forwarded
  - `test_passthrough_blocked_host_returns_403` - Allowlist enforcement
  - `test_passthrough_invalid_url_returns_400` - Invalid URL handling
  - `test_passthrough_empty_path_returns_404` - Empty path falls to fallback
  - `test_passthrough_upstream_error_returned` - 500 passthrough
  - `test_passthrough_upstream_404_returned` - 404 passthrough
  - `test_health_endpoint_still_works` - Health check coexistence
  - `test_passthrough_put_method` - PUT method support
  - `test_passthrough_delete_method` - DELETE method support
  - `test_passthrough_response_headers_forwarded` - Response headers pass through
  - `test_passthrough_empty_allowlist_allows_all` - Empty allowlist = allow all
  - `test_passthrough_body_forwarded` - Request body forwarded correctly

### Wiremock Testing Pattern
```rust
let mock_server = MockServer::start().await;

Mock::given(matchers::method("POST"))
    .and(matchers::path("/api"))
    .and(matchers::header("Authorization", "Bearer token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
    .mount(&mock_server)
    .await;
```

### Test Router Helper Pattern
```rust
fn create_test_router_with_allowed(allowed_hosts: Vec<String>) -> Router {
    let config = ProxyConfig { ... };
    let state = Arc::new(AppState {
        config,
        client: reqwest::Client::builder().timeout(...).build().unwrap(),
    });
    create_router(state)
}
```

### Empty Path Routing Behavior
`/p/` with empty wildcard falls through to fallback handler, returns 404 (no upstream configured).
Axum's `{*upstream_url}` wildcard requires at least one character.

### Test Count
- Previous: 29 tests in proxy_tests.rs
- Added: 15 new passthrough tests  
- Total: 44 tests in proxy_tests.rs

## 2026-02-01 Session Complete

### Final Implementation Summary
- All 6 implementation tasks completed
- All 11 verification checkboxes passed
- Total: 17/17 checkboxes complete

### Key Implementation Details
1. **Dependencies**: reqwest (rustls-tls, json), url, wiremock added to workspace
2. **Config Changes**: `upstream_url` now `Option<String>`, added `allowed_hosts: Vec<String>`
3. **Passthrough Module**: `UpstreamTarget` struct with `from_path()` and `is_allowed()` methods
4. **Server Rewrite**: `/p/*` routing with `dynamic_proxy_handler`, hop-by-hop header stripping
5. **Tests**: 44 proxy integration tests, 252 lib unit tests

### Clippy Fixes Required
- `uninlined_format_args`: Use `{var}` instead of `{}", var` in format strings
- `manual_strip`: Use `strip_prefix()` instead of `starts_with()` + slice

### Test Counts
- Proxy integration tests: 44 (was 29, added 15)
- Lib unit tests: 252 (includes 19 passthrough tests)
