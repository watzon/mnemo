# Anthropic Support - Decisions

## 2026-01-31 Task: Architecture Decisions

### Provider Detection Strategy
- **Decision**: Heuristic-based detection with URL fallback
- **Rationale**: No configuration needed, works with dynamic passthrough
- **Trade-off**: May misdetect edge cases, but fail-open handles gracefully

### Memory Format
- **Decision**: XML format for both providers
- **Rationale**: Consistent format, works well with both APIs
- **Format**: `<mnemo-memories><memory timestamp="..." type="...">content</memory></mnemo-memories>`

### Error Handling
- **Decision**: Fail-open - pass request through unmodified on any error
- **Rationale**: Never block user requests, memory is enhancement not requirement

### Tool Calls / Images / Thinking
- **Decision**: Passthrough without modification
- **Rationale**: These are provider-specific features, don't interfere
