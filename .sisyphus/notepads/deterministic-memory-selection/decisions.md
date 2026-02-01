# Decisions

## 2026-02-01 Planning Phase
- Deterministic mode: Opt-in via config (default disabled)
- Score quantization: 2 decimal places default, configurable
- Topic overlap: Scoring factor (additive boost), weight 0.1 default
- Tie-breaking: timestamp (older wins) → UUID
- Use existing Memory.entities field (no schema migration)
