# Dynamic URL Passthrough for Nova Memory Proxy

## TL;DR

> **Quick Summary**: Implement dynamic upstream URL passthrough via `/p/{url}` path pattern, eliminating mandatory upstream configuration. Requests to `/p/https://api.openai.com/v1/chat/completions` proxy directly to that URL.
> 
> **Deliverables**:
> - New `proxy/passthrough.rs` module for URL parsing and host validation
> - Modified `config/mod.rs` with optional `upstream_url` and new `allowed_hosts`
> - Rewritten `proxy/server.rs` with Axum routing and reqwest forwarding
> - Updated `config.example.toml` with new options
> - Comprehensive integration tests
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 5

---

## Context

### Original Request
User wants dynamic URL passthrough where the target API URL is encoded in the request path itself (`/p/https://api.openai.com/v1/...`) rather than configured statically. This enables a single proxy instance to handle multiple LLM providers without reconfiguration.

### Interview Summary
**Key Discussions**:
- Breaking changes allowed - greenfield project, no backward compatibility constraints
- Security via allowlist - `allowed_hosts` config restricts which hosts can be proxied
- Query strings pass through naturally - `?stream=true` goes to upstream
- Memory injection is out of scope - stub with TODO for this plan

**Research Findings**:
- Current proxy is a stub returning 501 Not Implemented
- Tests use `tower::ServiceExt::oneshot` pattern
- No reqwest or wiremock in dependencies yet
- Rust 2024 edition, MSRV 1.85

### Metis Review
**Identified Gaps** (addressed):
- URL parsing edge cases: Added comprehensive test cases
- Security open redirect: Strict allowlist validation required
- Header leakage: Explicit hop-by-hop header stripping
- Edge cases: Fragment stripping, IPv6 support, userinfo removal

---

## Work Objectives

### Core Objective
Enable dynamic upstream URL passthrough via `/p/{url}` path pattern with security controls via host allowlist.

### Concrete Deliverables
- `crates/nova-memory/src/proxy/passthrough.rs` - URL parsing, validation, allowlist checking
- `crates/nova-memory/src/proxy/server.rs` - Rewritten with proper routing
- `crates/nova-memory/src/config/mod.rs` - Schema changes
- `crates/nova-memory/Cargo.toml` - New dependencies
- `Cargo.toml` (workspace) - Workspace dependency additions
- `config.example.toml` - Updated documentation
- `crates/nova-memory/tests/proxy_tests.rs` - Passthrough tests (new module)

### Definition of Done
- [x] `cargo test -p nova-memory` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] Dynamic passthrough to httpbin.org works manually
- [x] Blocked hosts return 403 Forbidden
- [x] Invalid URLs return 400 Bad Request
- [x] Health endpoint unchanged

### Must Have
- URL extraction from `/p/*` path
- Host validation against `allowed_hosts`
- Proper HTTP header forwarding (strip hop-by-hop)
- Query string passthrough
- Timeout handling from config
- JSON error responses

### Must NOT Have (Guardrails)
- Memory injection implementation (stub only)
- Response capture implementation (stub only)
- Authentication/authorization for proxy itself
- Rate limiting
- Retry logic for failed requests
- Connection pooling configuration
- WebSocket upgrade support
- Request body modification (forward unchanged)
- Load balancing to multiple upstreams

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES (Rust native tests)
- **User wants tests**: YES (follow existing patterns)
- **Framework**: Rust built-in + wiremock for mock servers

### Testing Approach
Tests will use:
- `tower::ServiceExt::oneshot` for handler unit tests (existing pattern)
- `wiremock` for mock upstream server tests
- No external services required

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately):
├── Task 1: Add dependencies to Cargo.toml files
└── (No other independent tasks)

Wave 2 (After Wave 1):
├── Task 2: Modify config schema
└── Task 3: Create passthrough module (can start after deps)

Wave 3 (After Wave 2):
├── Task 4: Rewrite server.rs (depends on 2, 3)
└── Task 5: Update config.example.toml (depends on 2)

Wave 4 (After Wave 3):
└── Task 6: Add integration tests (depends on 4)

Critical Path: 1 → 2 → 4 → 6
Parallel Speedup: ~25% (limited parallelization due to dependencies)
```

### Dependency Matrix

| Task | Depends On | Blocks | Can Parallelize With |
|------|------------|--------|---------------------|
| 1 | None | 2, 3 | None |
| 2 | 1 | 4, 5 | 3 |
| 3 | 1 | 4 | 2 |
| 4 | 2, 3 | 6 | 5 |
| 5 | 2 | None | 4 |
| 6 | 4 | None | None |

---

## TODOs

- [x] 1. Add workspace dependencies (reqwest, wiremock, url)

  **What to do**:
  - Add `reqwest` with `rustls-tls` and `json` features to workspace `Cargo.toml`
  - Add `wiremock` for testing to workspace `Cargo.toml`
  - Add `url` crate for URL parsing to workspace `Cargo.toml`
  - Add these dependencies to `crates/nova-memory/Cargo.toml`

  **Must NOT do**:
  - Do not use `native-tls` feature (use `rustls-tls` for consistency)
  - Do not add any other dependencies beyond these three

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple config file modifications, no complex logic
  - **Skills**: `[]`
    - No special skills needed for Cargo.toml edits

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (alone)
  - **Blocks**: Tasks 2, 3
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `Cargo.toml:1-80` - Workspace dependency declaration pattern (note the `{ workspace = true }` pattern in crate Cargo.toml)

  **Files to modify**:
  - `Cargo.toml` (workspace root) - Add to `[workspace.dependencies]` section
  - `crates/nova-memory/Cargo.toml` - Add to `[dependencies]` and `[dev-dependencies]`

  **Acceptance Criteria**:

  ```bash
  # Verify dependencies resolve
  cargo check -p nova-memory
  # Assert: Exit code 0, no dependency resolution errors
  
  # Verify reqwest is available
  cargo build -p nova-memory 2>&1 | grep -v "reqwest"
  # Assert: No "unresolved import" errors for reqwest
  ```

  **Commit**: YES
  - Message: `feat(proxy): add reqwest, url, wiremock dependencies`
  - Files: `Cargo.toml`, `crates/nova-memory/Cargo.toml`

---

- [x] 2. Modify ProxyConfig schema

  **What to do**:
  - Change `upstream_url: String` to `upstream_url: Option<String>` in `ProxyConfig`
  - Add `allowed_hosts: Vec<String>` field with `#[serde(default)]`
  - Update `Default` impl for `ProxyConfig`
  - Update existing tests to handle `Option<String>`
  - Add new tests for `allowed_hosts` deserialization

  **Must NOT do**:
  - Do not modify other config structs (`StorageConfig`, `RouterConfig`, `EmbeddingConfig`)
  - Do not change the config file format in breaking ways beyond `upstream_url`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Straightforward struct modifications with clear patterns
  - **Skills**: `[]`
    - Standard Rust, no special domain knowledge needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Task 3)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: Task 1

  **References**:

  **Pattern References**:
  - `crates/nova-memory/src/config/mod.rs:67-92` - Current `ProxyConfig` struct definition
  - `crates/nova-memory/src/config/mod.rs:83-91` - Current `Default` impl pattern
  - `crates/nova-memory/src/config/mod.rs:22-47` - `StorageConfig` as example of field with `#[serde(default)]`

  **Test References**:
  - `crates/nova-memory/src/config/mod.rs:178-191` - `test_config_default()` pattern
  - `crates/nova-memory/src/config/mod.rs:193-240` - TOML deserialization test pattern
  - `crates/nova-memory/src/config/mod.rs:242-256` - Partial deserialization test pattern

  **Acceptance Criteria**:

  ```bash
  # Run config module tests
  cargo test -p nova-memory config -- --test-threads=1
  # Assert: All tests pass
  
  # Verify compilation
  cargo check -p nova-memory
  # Assert: Exit code 0
  ```

  **Specific test cases to add**:
  - Test `upstream_url` is `None` when not provided in TOML
  - Test `upstream_url` is `Some(...)` when provided
  - Test `allowed_hosts` defaults to empty `Vec`
  - Test `allowed_hosts` parses from TOML array

  **Commit**: YES
  - Message: `feat(config): make upstream_url optional, add allowed_hosts`
  - Files: `crates/nova-memory/src/config/mod.rs`

---

- [x] 3. Create passthrough module

  **What to do**:
  - Create new file `crates/nova-memory/src/proxy/passthrough.rs`
  - Implement `UpstreamTarget` struct with `url: Url` and `host: String`
  - Implement `UpstreamTarget::from_path(path: &str, query: Option<&str>) -> Result<Self>`
  - Implement `UpstreamTarget::is_allowed(config: &ProxyConfig) -> bool`
  - Handle URL normalization (single slash after scheme)
  - Handle edge cases: fragments (strip), userinfo (strip with warning), non-HTTP schemes (reject)
  - Add `pub use passthrough::*;` to `proxy/mod.rs`

  **Must NOT do**:
  - Do not implement memory injection logic
  - Do not implement response capture logic
  - Do not modify `error.rs` (use existing error types)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: New module with clear spec, moderate complexity
  - **Skills**: `[]`
    - Standard Rust URL handling

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Task 2)
  - **Blocks**: Task 4
  - **Blocked By**: Task 1

  **References**:

  **Pattern References**:
  - `crates/nova-memory/src/proxy/mod.rs:1-18` - Module structure and re-exports
  - `crates/nova-memory/src/error.rs` - `NovaError` enum for error handling

  **API References**:
  - `url` crate: `Url::parse()`, `url.host_str()`, `url.set_query()`, `url.set_fragment()`

  **Acceptance Criteria**:

  ```bash
  # Verify module compiles
  cargo check -p nova-memory
  # Assert: Exit code 0
  
  # Unit tests (will be inline in the module)
  cargo test -p nova-memory passthrough -- --test-threads=1
  # Assert: All tests pass
  ```

  **Unit tests to include in module**:
  ```rust
  #[cfg(test)]
  mod tests {
      // test_from_path_basic_https
      // test_from_path_single_slash_normalization  
      // test_from_path_with_query_string
      // test_from_path_strips_fragment
      // test_from_path_strips_userinfo
      // test_from_path_rejects_non_http
      // test_from_path_rejects_invalid_url
      // test_is_allowed_empty_allowlist
      // test_is_allowed_exact_match
      // test_is_allowed_subdomain_match
      // test_is_allowed_blocked_host
  }
  ```

  **Commit**: YES
  - Message: `feat(proxy): add passthrough module for URL parsing and validation`
  - Files: `crates/nova-memory/src/proxy/passthrough.rs`, `crates/nova-memory/src/proxy/mod.rs`

---

- [x] 4. Rewrite proxy server with routing

  **What to do**:
  - Create `AppState` struct with `config: ProxyConfig` and `client: reqwest::Client`
  - Add route `/p/*upstream_url` using `axum::routing::any` for dynamic passthrough
  - Add fallback handler for configured upstream (when `upstream_url` is `Some`)
  - Implement `dynamic_proxy_handler` that:
    - Extracts URL from path using `UpstreamTarget::from_path`
    - Validates against allowlist using `is_allowed`
    - Returns 403 if blocked, 400 if invalid URL
    - Forwards request with proper header handling
  - Implement `configured_proxy_handler` that:
    - Returns 404 with helpful message if `upstream_url` is `None`
    - Constructs full URL from base + request path
    - Forwards request
  - Implement `forward_request` shared function:
    - Strip hop-by-hop headers: `host`, `connection`, `keep-alive`, `transfer-encoding`
    - Read body bytes
    - Build reqwest request
    - Forward and return response
    - Log TODO for memory injection point
    - Log TODO for response capture point
  - Add startup log messages showing configuration

  **Must NOT do**:
  - Do not implement actual memory injection (log TODO)
  - Do not implement actual response capture (log TODO)
  - Do not implement streaming passthrough yet (use simple body forwarding)
  - Do not implement retry logic
  - Do not implement WebSocket upgrade handling

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Core functionality, multiple handlers, state management
  - **Skills**: `[]`
    - Standard Axum patterns

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Task 5)
  - **Blocks**: Task 6
  - **Blocked By**: Tasks 2, 3

  **References**:

  **Pattern References**:
  - `crates/nova-memory/src/proxy/server.rs:1-122` - Current server structure (REPLACE entirely)
  - `crates/nova-memory/src/proxy/error.rs:176-190` - `handle_upstream_error` pattern

  **API References**:
  - Axum 0.8: `Router::new()`, `routing::any()`, `routing::get()`, `.fallback()`, `.with_state()`
  - Axum extractors: `State<Arc<AppState>>`, `Path<String>`, `Uri`, `Method`, `HeaderMap`, `Body`
  - reqwest: `Client::builder()`, `.timeout()`, `.request()`, `.headers()`, `.body()`, `.send()`

  **Test References**:
  - `crates/nova-memory/tests/proxy_tests.rs:71-127` - Health endpoint test pattern

  **Acceptance Criteria**:

  ```bash
  # Verify compilation
  cargo check -p nova-memory
  # Assert: Exit code 0
  
  # Verify existing health test still passes
  cargo test -p nova-memory health_endpoint -- --test-threads=1
  # Assert: All health tests pass
  ```

  **Commit**: YES
  - Message: `feat(proxy): implement dynamic URL passthrough with /p/* routing`
  - Files: `crates/nova-memory/src/proxy/server.rs`

---

- [x] 5. Update config.example.toml

  **What to do**:
  - Update `[proxy]` section documentation
  - Change `upstream_url` to be commented out with note about optional usage
  - Add `allowed_hosts` with examples and security documentation
  - Add warnings about empty allowlist

  **Must NOT do**:
  - Do not modify other sections (`[storage]`, `[router]`, `[embedding]`)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Documentation update only
  - **Skills**: `[]`
    - No special skills needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Task 4)
  - **Blocks**: None
  - **Blocked By**: Task 2

  **References**:

  **Pattern References**:
  - `config.example.toml:29-56` - Current `[proxy]` section format and documentation style

  **Acceptance Criteria**:

  ```bash
  # Verify TOML is valid
  cargo run -p nova-memory -- --config config.example.toml --help 2>&1 || true
  # Assert: No TOML parse errors (may fail for other reasons, that's ok)
  
  # Manual verification: file contains expected sections
  grep -q "allowed_hosts" config.example.toml
  # Assert: Exit code 0
  ```

  **Commit**: YES
  - Message: `docs(config): update example with allowed_hosts and optional upstream_url`
  - Files: `config.example.toml`

---

- [x] 6. Add passthrough integration tests

  **What to do**:
  - Add new test module `passthrough_tests` to `crates/nova-memory/tests/proxy_tests.rs`
  - Use `wiremock::MockServer` to create mock upstream
  - Test cases:
    - Basic POST passthrough to mock server
    - GET with query string passthrough
    - Headers forwarded correctly (auth header)
    - Hop-by-hop headers stripped
    - Host not in allowlist returns 403
    - Invalid URL returns 400
    - Empty path after /p/ returns 400
    - Upstream 4xx/5xx passed through
    - Configured upstream fallback works (when `upstream_url` set)
    - Health endpoint unaffected

  **Must NOT do**:
  - Do not require external services (use wiremock only)
  - Do not add tests that need `--test-threads=1` (proxy tests don't use ML)
  - Do not modify existing test modules (add new module)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Comprehensive test suite, multiple scenarios
  - **Skills**: `[]`
    - Standard Rust testing patterns

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (final)
  - **Blocks**: None
  - **Blocked By**: Task 4

  **References**:

  **Pattern References**:
  - `crates/nova-memory/tests/proxy_tests.rs:68-127` - Module structure with `mod` blocks
  - `crates/nova-memory/tests/proxy_tests.rs:71-86` - `oneshot` test pattern
  - `crates/nova-memory/tests/integration_test.rs:155-165` - Mock server creation pattern

  **API References**:
  - wiremock: `MockServer::start()`, `Mock::given()`, `matchers::*`, `ResponseTemplate`
  - tower: `ServiceExt::oneshot()`

  **Acceptance Criteria**:

  ```bash
  # Run all proxy tests
  cargo test -p nova-memory proxy -- --test-threads=1
  # Assert: All tests pass, including new passthrough_tests module
  
  # Verify test count increased
  cargo test -p nova-memory proxy -- --test-threads=1 2>&1 | grep "test result"
  # Assert: Shows more tests than before (was ~20, should be ~35+)
  ```

  **Commit**: YES
  - Message: `test(proxy): add comprehensive passthrough integration tests`
  - Files: `crates/nova-memory/tests/proxy_tests.rs`

---

## Commit Strategy

| After Task | Message | Files | Pre-commit Check |
|------------|---------|-------|------------------|
| 1 | `feat(proxy): add reqwest, url, wiremock dependencies` | Cargo.toml (2 files) | `cargo check` |
| 2 | `feat(config): make upstream_url optional, add allowed_hosts` | config/mod.rs | `cargo test config` |
| 3 | `feat(proxy): add passthrough module for URL parsing and validation` | passthrough.rs, mod.rs | `cargo test passthrough` |
| 4 | `feat(proxy): implement dynamic URL passthrough with /p/* routing` | server.rs | `cargo check` |
| 5 | `docs(config): update example with allowed_hosts and optional upstream_url` | config.example.toml | N/A |
| 6 | `test(proxy): add comprehensive passthrough integration tests` | proxy_tests.rs | `cargo test proxy` |

---

## Success Criteria

### Verification Commands
```bash
# Full test suite
cargo test --workspace -- --test-threads=1
# Expected: All tests pass

# Clippy
cargo clippy --workspace -- -D warnings
# Expected: No warnings

# Manual smoke test (after starting daemon)
curl -X POST http://localhost:9999/p/https://httpbin.org/post \
  -H "Content-Type: application/json" \
  -d '{"test": "data"}'
# Expected: httpbin echoes request back

curl http://localhost:9999/p/https://blocked.example.com/
# Expected: 403 Forbidden (if not in allowed_hosts)

curl http://localhost:9999/health
# Expected: {"status": "ok"}
```

### Final Checklist
- [x] All "Must Have" present
- [x] All "Must NOT Have" absent (no memory injection, no response capture, etc.)
- [x] All tests pass
- [x] Clippy passes with `-D warnings`
- [x] Config changes documented
