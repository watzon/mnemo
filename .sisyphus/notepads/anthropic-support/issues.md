# Anthropic Support - Issues

## 2026-01-31 Task: Known Issues

### Pre-existing Issues (Unrelated)
- `determinism_tests.rs` has compilation errors referencing non-existent `DeterministicConfig`
- This is a pre-existing issue, not caused by Anthropic support work

### Current State
- Tasks 1-6 complete (Provider enum, LLMProvider trait, OpenAI/Anthropic providers)
- Tasks 7-8 in progress (wiring into server.rs)
- server.rs has imports added but TODOs at lines 240 and 304 need implementation
