# Dynamic Passthrough Decisions

## Architecture Decisions (from plan)

### URL Passthrough Pattern
- `/p/{url}` path pattern for dynamic upstream
- Example: `/p/https://api.openai.com/v1/chat/completions`
- Security via `allowed_hosts` allowlist

### Dependencies
- `reqwest` with `rustls-tls` (NOT native-tls) for HTTP client
- `url` crate for URL parsing
- `wiremock` for testing (dev-dependency)

### Breaking Changes Allowed
- `upstream_url` changes from required `String` to `Option<String>`
- This is greenfield, no backward compatibility needed
