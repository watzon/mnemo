# Deterministic Memory Selection

## TL;DR

> **Quick Summary**: Add opt-in deterministic retrieval mode that uses score quantization, topic overlap scoring, and stable tie-breaking to produce consistent memory orderings for improved LLM cache hit rates.
> 
> **Deliverables**:
> - `DeterministicConfig` settings in `RouterConfig`
> - Modified `RetrievalPipeline` with quantized scoring and topic overlap
> - Deterministic sorting with timestamp + UUID tiebreakers
> - Integration tests for deterministic behavior
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 2 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 5

---

## Context

### Original Request
User wants deterministic memory selection to improve LLM provider cache hit rates (e.g., Anthropic's prompt caching). Similar queries currently produce slightly different memory orderings due to floating-point variance, causing cache misses.

### Interview Summary
**Key Discussions**:
- Deterministic mode: Opt-in via config (`retrieval.deterministic = true`)
- Score quantization: Configurable, default 2 decimal places
- Topic bucketing: Scoring factor (soft influence via entity overlap)
- Tie-breaking: By timestamp (older wins), then by UUID
- Topic overlap weight: Configurable, default 0.1

**Research Findings**:
- Router already extracts topics via NER (`RouterOutput.topics`, `RouterOutput.entities`)
- Memory struct has `entities` field - use this as topic proxy (no schema migration)
- Current scoring: `final_score = similarity * 0.7 + effective_weight * 0.3`
- Current sorting uses `partial_cmp` with no deterministic tie-breaking
- Both `retrieve()` and `retrieve_by_embedding()` need updating

### Metis Review
**Identified Gaps** (addressed):
- Memory.entities as topic proxy (avoiding schema migration)
- Apply determinism to BOTH retrieval methods
- Handle NaN/Inf explicitly (push to end)
- Use `f32::total_cmp()` for deterministic float ordering
- Secondary tiebreaker by UUID when timestamps equal

---

## Work Objectives

### Core Objective
Implement opt-in deterministic memory selection using stable scoring (quantization + tiebreakers) and topic overlap as a scoring factor to improve LLM provider cache hit rates.

### Concrete Deliverables
- `DeterministicConfig` struct with `enabled`, `decimal_places`, `topic_overlap_weight`
- Extended `RouterConfig` with deterministic settings
- Modified `RetrievedMemory::new()` accepting query entities
- Modified `RetrievalPipeline` sorting with deterministic comparator
- Updated `config.example.toml` with new settings
- Integration tests in `tests/retrieval_determinism_tests.rs`

### Definition of Done
- [ ] `cargo test --workspace -- --test-threads=1` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] Same query + same memories → byte-identical ordering
- [ ] `deterministic = false` behavior unchanged from baseline

### Must Have
- Opt-in via config (default disabled)
- Score quantization with configurable precision
- Topic/entity overlap as scoring factor
- Deterministic tie-breaking (timestamp then UUID)
- Tests proving determinism

### Must NOT Have (Guardrails)
- Schema migration for Memory struct
- New `topics` field on Memory (use existing `entities`)
- Changes to RouterOutput structure
- Changes to base scoring weights (0.7/0.3)
- Behavior changes when deterministic = false
- Config hot-reload handling

---

## Verification Strategy (MANDATORY)

### Test Decision
- **Infrastructure exists**: YES (10 test files in tests/)
- **User wants tests**: YES (tests after implementation)
- **Framework**: cargo test (Rust standard)

### If Automated Verification Only (NO User Intervention)

Each TODO includes EXECUTABLE verification procedures:

**For Rust code changes** (using Bash cargo):
```bash
# Agent runs:
cargo test --workspace -- --test-threads=1
# Assert: Exit code 0, all tests pass

cargo clippy --workspace -- -D warnings
# Assert: Exit code 0, no warnings
```

**Evidence Requirements:**
- Test output captured showing pass/fail counts
- Clippy output showing zero warnings

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately):
├── Task 1: Add DeterministicConfig to config module
└── Task 2: Add topic_overlap_score() utility function

Wave 2 (After Wave 1):
└── Task 3: Modify RetrievalPipeline with deterministic logic

Wave 3 (After Wave 2):
└── Task 4: Update config.example.toml documentation

Wave 4 (After Wave 3):
└── Task 5: Add integration tests for deterministic behavior

Critical Path: Task 1 → Task 3 → Task 5
Parallel Speedup: ~30% faster than sequential
```

### Dependency Matrix

| Task | Depends On | Blocks | Can Parallelize With |
|------|------------|--------|---------------------|
| 1 | None | 3 | 2 |
| 2 | None | 3 | 1 |
| 3 | 1, 2 | 4, 5 | None |
| 4 | 3 | None | 5 |
| 5 | 3 | None | 4 |

### Agent Dispatch Summary

| Wave | Tasks | Recommended Agents |
|------|-------|-------------------|
| 1 | 1, 2 | delegate_task(category="quick", load_skills=["crystal-lang"], run_in_background=true) |
| 2 | 3 | delegate_task(category="unspecified-high", load_skills=[], run_in_background=false) |
| 3 | 4, 5 | delegate_task(category="quick", ..., run_in_background=true) parallel |

---

## TODOs

- [x] 1. Add DeterministicConfig to RouterConfig

  **What to do**:
  - Create `DeterministicConfig` struct with fields:
    - `enabled: bool` (default: false)
    - `decimal_places: u8` (default: 2)
    - `topic_overlap_weight: f32` (default: 0.1)
  - Add `deterministic: DeterministicConfig` field to `RouterConfig`
  - Add default functions for each setting
  - Update `RouterConfig::default()` implementation
  - Add tests for config parsing with new fields

  **Must NOT do**:
  - Add to a separate config file
  - Add hot-reload logic
  - Change existing RouterConfig field names

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small, focused config addition, single file change
  - **Skills**: `[]`
    - No special skills needed for Rust config work
  - **Skills Evaluated but Omitted**:
    - `crystal-lang`: This is Rust, not Crystal

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 2)
  - **Blocks**: Task 3
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/mnemo/src/config/mod.rs:112-133` - RouterConfig struct pattern, follow same style for nested config
  - `crates/mnemo/src/config/mod.rs:135-141` - Default function pattern (`default_max_memories`)

  **Test References**:
  - `crates/mnemo/src/config/mod.rs:201-225` - TOML deserialization test pattern

  **WHY Each Reference Matters**:
  - The config module uses a specific pattern: struct with `#[serde(default = "fn_name")]`, separate default functions, and comprehensive TOML tests

  **Acceptance Criteria**:

  ```bash
  # Agent runs:
  cargo build -p mnemo
  # Assert: Exit code 0, compiles successfully

  cargo test -p mnemo config -- --test-threads=1
  # Assert: Exit code 0, config tests pass
  ```

  **Commit**: YES
  - Message: `feat(config): add DeterministicConfig for stable memory retrieval`
  - Files: `crates/mnemo/src/config/mod.rs`
  - Pre-commit: `cargo test -p mnemo config -- --test-threads=1`

---

- [x] 2. Add topic_overlap_score utility function

  **What to do**:
  - Add `topic_overlap_score()` function to `memory/retrieval.rs`:
    ```rust
    fn topic_overlap_score(query_entities: &[String], memory_entities: &[String]) -> f32 {
        if query_entities.is_empty() || memory_entities.is_empty() {
            return 0.0;
        }
        let query_set: HashSet<_> = query_entities.iter()
            .map(|s| s.to_lowercase())
            .collect();
        let matches = memory_entities.iter()
            .filter(|e| query_set.contains(&e.to_lowercase()))
            .count();
        matches as f32 / query_entities.len().max(1) as f32
    }
    ```
  - Add unit tests for the function
  - Handle empty inputs gracefully (return 0.0)

  **Must NOT do**:
  - Use stemming or lemmatization (keep it simple)
  - Change Memory struct
  - Add external dependencies

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single utility function, straightforward logic
  - **Skills**: `[]`
    - Standard Rust, no special skills needed
  - **Skills Evaluated but Omitted**:
    - None relevant

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 1)
  - **Blocks**: Task 3
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/mnemo/src/memory/retrieval.rs:225-239` - `cosine_similarity()` function pattern (same file, similar utility function style)

  **Test References**:
  - `crates/mnemo/src/memory/retrieval.rs:242-300` - Unit test pattern for retrieval utilities

  **WHY Each Reference Matters**:
  - Follow the existing utility function pattern in the same file for consistency

  **Acceptance Criteria**:

  ```bash
  # Agent runs:
  cargo test -p mnemo retrieval::tests::test_topic_overlap -- --test-threads=1
  # Assert: Exit code 0, test passes

  # Verify function handles edge cases:
  # - Empty query_entities → returns 0.0
  # - Empty memory_entities → returns 0.0
  # - Case-insensitive matching
  # - Partial overlap calculated correctly
  ```

  **Commit**: YES
  - Message: `feat(retrieval): add topic_overlap_score utility for entity matching`
  - Files: `crates/mnemo/src/memory/retrieval.rs`
  - Pre-commit: `cargo test -p mnemo retrieval -- --test-threads=1`

---

- [x] 3. Modify RetrievalPipeline with deterministic scoring and sorting

  **What to do**:
  - Add `query_entities: Option<Vec<String>>` parameter to `RetrievedMemory::new()` and related methods
  - Add `deterministic_config: Option<DeterministicConfig>` to `RetrievalConfig`
  - Implement `quantize_score()` helper:
    ```rust
    fn quantize_score(score: f32, decimal_places: u8) -> f32 {
        let multiplier = 10_f32.powi(decimal_places as i32);
        (score * multiplier).round() / multiplier
    }
    ```
  - Modify `final_score` calculation to include topic overlap:
    ```rust
    // When deterministic enabled:
    let topic_score = topic_overlap_score(&query_entities, &memory.entities);
    let base_score = similarity * sim_weight + weight * rerank_weight;
    let topic_boost = topic_score * topic_overlap_weight;
    let raw_final = base_score + topic_boost;
    let final_score = quantize_score(raw_final, decimal_places);
    ```
  - Replace sorting in `retrieve()` and `retrieve_by_embedding()` with deterministic comparator:
    ```rust
    if deterministic_enabled {
        results.sort_by(|a, b| {
            // Primary: quantized final_score descending
            b.final_score.total_cmp(&a.final_score)
                // Secondary: created_at ascending (older wins)
                .then_with(|| a.memory.created_at.cmp(&b.memory.created_at))
                // Tertiary: id ascending (deterministic tiebreaker)
                .then_with(|| a.memory.id.cmp(&b.memory.id))
        });
    } else {
        // Original behavior
        results.sort_by(|a, b| {
            b.final_score.partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    ```
  - Handle NaN/Inf: `total_cmp()` handles this correctly (NaN sorts to end)

  **Must NOT do**:
  - Change behavior when deterministic = false
  - Modify the base 0.7/0.3 weight ratio
  - Add new fields to Memory struct
  - Break existing method signatures (add optional params)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Core logic change touching multiple methods, needs careful attention to preserve existing behavior
  - **Skills**: `[]`
    - Standard Rust, no special skills needed
  - **Skills Evaluated but Omitted**:
    - None relevant

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential)
  - **Blocks**: Task 4, Task 5
  - **Blocked By**: Task 1, Task 2

  **References**:

  **Pattern References**:
  - `crates/mnemo/src/memory/retrieval.rs:27-46` - `RetrievedMemory::new()` current implementation
  - `crates/mnemo/src/memory/retrieval.rs:48-70` - `RetrievalConfig` struct pattern
  - `crates/mnemo/src/memory/retrieval.rs:149-153` - Current sorting logic to modify

  **API/Type References**:
  - `crates/mnemo/src/memory/types.rs` - Memory struct with `entities`, `created_at`, `id` fields
  - `crates/mnemo/src/config/mod.rs:112-133` - RouterConfig to import DeterministicConfig from

  **Test References**:
  - `crates/mnemo/tests/integration_test.rs` - Integration test patterns for retrieval

  **External References**:
  - Rust docs: `f32::total_cmp()` - deterministic float comparison including NaN handling

  **WHY Each Reference Matters**:
  - `RetrievedMemory::new()`: Must extend signature, not replace
  - `RetrievalConfig`: Where to add deterministic settings
  - Sorting logic: Critical modification point
  - Memory struct: Need to access `entities`, `created_at`, `id` for scoring and tiebreaking

  **Acceptance Criteria**:

  ```bash
  # Agent runs:
  cargo build -p mnemo
  # Assert: Exit code 0

  cargo test -p mnemo retrieval -- --test-threads=1
  # Assert: Exit code 0, all retrieval tests pass

  cargo clippy -p mnemo -- -D warnings
  # Assert: Exit code 0, no warnings
  ```

  **Commit**: YES
  - Message: `feat(retrieval): implement deterministic scoring with topic overlap and stable sorting`
  - Files: `crates/mnemo/src/memory/retrieval.rs`
  - Pre-commit: `cargo test -p mnemo -- --test-threads=1`

---

- [x] 4. Update config.example.toml with deterministic settings

  **What to do**:
  - Add deterministic settings to `[router]` section:
    ```toml
    [router]
    strategy = "semantic"
    max_memories = 10
    relevance_threshold = 0.7

    # Deterministic retrieval for improved LLM cache hit rates
    # When enabled, similar queries produce identical memory orderings
    [router.deterministic]
    enabled = false           # Opt-in, default false
    decimal_places = 2        # Score quantization precision (1-4)
    topic_overlap_weight = 0.1  # Weight for entity/topic matching (0.0-1.0)
    ```
  - Add comments explaining the purpose and trade-offs

  **Must NOT do**:
  - Change existing config values
  - Add settings that don't exist in code

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Documentation update, simple file edit
  - **Skills**: `[]`
    - No special skills needed
  - **Skills Evaluated but Omitted**:
    - None relevant

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Task 5)
  - **Blocks**: None
  - **Blocked By**: Task 3

  **References**:

  **Pattern References**:
  - `config.example.toml` - Existing config documentation style

  **WHY Each Reference Matters**:
  - Follow existing documentation style for consistency

  **Acceptance Criteria**:

  ```bash
  # Agent runs:
  # Verify TOML is valid
  cat config.example.toml | toml-test 2>/dev/null || python3 -c "import tomllib; tomllib.load(open('config.example.toml', 'rb'))"
  # Assert: Exit code 0, valid TOML

  # Verify new section exists
  grep -A 5 '\[router.deterministic\]' config.example.toml
  # Assert: Output shows the new section with enabled, decimal_places, topic_overlap_weight
  ```

  **Commit**: YES
  - Message: `docs(config): add deterministic retrieval settings to example config`
  - Files: `config.example.toml`
  - Pre-commit: None

---

- [x] 5. Add integration tests for deterministic behavior

  **What to do**:
  - Create `crates/mnemo/tests/determinism_tests.rs`
  - Add test: `test_deterministic_same_query_same_order`
    - Create store with multiple memories
    - Run same query twice with deterministic=true
    - Assert byte-identical `Vec<RetrievedMemory>` ordering
  - Add test: `test_deterministic_tiebreaker_by_timestamp`
    - Create memories with scores that quantize to same value
    - Assert older memory appears first
  - Add test: `test_deterministic_tiebreaker_by_id`
    - Create memories with same quantized score AND same timestamp
    - Assert consistent ordering by UUID
  - Add test: `test_topic_overlap_boosts_score`
    - Create memory with matching entities
    - Assert it ranks higher than non-matching memory
  - Add test: `test_nondeterministic_mode_unchanged`
    - Verify deterministic=false produces same behavior as baseline

  **Must NOT do**:
  - Create tests that require user interaction
  - Skip the `--test-threads=1` requirement
  - Modify existing test files

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Test file creation following established patterns
  - **Skills**: `[]`
    - Standard Rust testing, no special skills
  - **Skills Evaluated but Omitted**:
    - None relevant

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Task 4)
  - **Blocks**: None
  - **Blocked By**: Task 3

  **References**:

  **Pattern References**:
  - `crates/mnemo/tests/integration_test.rs` - Integration test structure, store setup patterns
  - `crates/mnemo/src/testing.rs` - Shared test utilities, SHARED_MEMORY_ROUTER singleton

  **Test References**:
  - `crates/mnemo/tests/weight_tests.rs` - Weight-related test patterns

  **WHY Each Reference Matters**:
  - Integration tests use tempdir pattern for isolated LanceDB
  - SHARED_MEMORY_ROUTER avoids model reload overhead
  - Weight tests show how to test scoring behavior

  **Acceptance Criteria**:

  ```bash
  # Agent runs:
  cargo test -p mnemo determinism -- --test-threads=1
  # Assert: Exit code 0, all 5 tests pass

  cargo test -p mnemo determinism::test_deterministic_same_query_same_order -- --test-threads=1 --nocapture
  # Assert: Test output shows two identical orderings
  ```

  **Commit**: YES
  - Message: `test(retrieval): add deterministic memory selection integration tests`
  - Files: `crates/mnemo/tests/determinism_tests.rs`
  - Pre-commit: `cargo test -p mnemo determinism -- --test-threads=1`

---

## Commit Strategy

| After Task | Message | Files | Verification |
|------------|---------|-------|--------------|
| 1 | `feat(config): add DeterministicConfig for stable memory retrieval` | config/mod.rs | cargo test config |
| 2 | `feat(retrieval): add topic_overlap_score utility for entity matching` | memory/retrieval.rs | cargo test retrieval |
| 3 | `feat(retrieval): implement deterministic scoring with topic overlap and stable sorting` | memory/retrieval.rs | cargo test retrieval |
| 4 | `docs(config): add deterministic retrieval settings to example config` | config.example.toml | TOML validation |
| 5 | `test(retrieval): add deterministic memory selection integration tests` | tests/determinism_tests.rs | cargo test determinism |

---

## Success Criteria

### Verification Commands
```bash
# Full test suite
cargo test --workspace -- --test-threads=1
# Expected: All tests pass

# Lint check
cargo clippy --workspace -- -D warnings
# Expected: No warnings

# Specific determinism tests
cargo test -p mnemo determinism -- --test-threads=1 --nocapture
# Expected: 5 tests pass, output shows deterministic behavior
```

### Final Checklist
- [ ] DeterministicConfig struct exists with all fields
- [ ] RouterConfig includes deterministic settings
- [ ] `retrieve()` and `retrieve_by_embedding()` support deterministic mode
- [ ] Score quantization implemented with configurable precision
- [ ] Topic overlap scoring implemented using Memory.entities
- [ ] Deterministic sorting uses `total_cmp()` with timestamp+UUID tiebreakers
- [ ] config.example.toml updated with new settings
- [ ] 5 integration tests pass
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] `cargo clippy` passes with no warnings
