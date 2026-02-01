# Dynamic Passthrough Issues

## Known Edge Cases (from Metis review)

### URL Parsing
- Fragment stripping required (no fragments to upstream)
- Userinfo removal with warning (security)
- Single slash normalization after scheme
- IPv6 support required

### Security Concerns
- Empty allowlist = allow all (documented warning needed)
- Open redirect risk mitigated by strict allowlist
- Hop-by-hop headers must be stripped
