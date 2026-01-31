# Nova Memory System — Work Plan

## TL;DR

> **Quick Summary**: Build an automatic, associative memory system that acts as a transparent HTTP proxy between chat clients and LLM APIs, automatically injecting relevant memories and capturing new ones from responses.
> 
> **Deliverables**:
> - Rust daemon that proxies OpenAI-compatible API requests
> - LanceDB-backed vector storage with three-tier architecture
> - DistilBERT-powered memory router for fast context retrieval
> - Weight-based organic memory system with decay and reinforcement
> - CLI for configuration and management
> 
> **Estimated Effort**: XL (4 phases, ~40+ tasks)
> **Parallel Execution**: YES - 4 waves per phase where possible
> **Critical Path**: Project Setup → Storage Layer → Embedding Pipeline → Router → Proxy Daemon

---

## Context

### Original Request
Build an automatic, associative memory system for AI agents that:
- Surfaces relevant context without explicit queries
- Handles storage constraints gracefully
- Behaves more like organic memory than a database

### Key Innovation
The system acts as a **transparent HTTP proxy/tunnel** between chat clients and LLM APIs. ALL conversation traffic flows through Nova Memory, solving the "opt-in problem" where agents forget to check memory.

### Interview Summary
**Key Decisions**:
- **Deployment**: Local-only (privacy-first), potential Pinecone later
- **Integration**: Daemon as tunnel/proxy (primary), MCP (v2), crate (v2)
- **Latency**: <250ms tolerance
- **Storage**: Configurable (hot ~10GB, warm 50-100GB, cold optional)
- **API Scope**: OpenAI-compatible only for v1
- **Streaming**: Tee the stream (real-time to client, buffer for ingestion)
- **Failure Mode**: Fail-open (pass through without memory)
- **Memory Schema**: Full (conversation ID, memory type enum, entities)
- **Injection**: XML-tagged block in system prompt
- **Config**: TOML format
- **Multi-user**: Single user only (v1)
- **Testing**: Tests after implementation

### Research Findings
| Component | Choice | Rationale |
|-----------|--------|-----------|
| Vector DB | LanceDB 0.23 | Only embedded option with HNSW + 10GB+ support |
| Router (fast) | DistilBERT-NER via Candle | 15-30ms, pure Rust |
| Embeddings | e5-small via fastembed-rs | 100% Top-5 accuracy, 16ms |
| Async Runtime | Tokio | Standard for Rust async |
| HTTP Framework | axum or hyper | High-performance, streaming support |

### Metis Review
**Identified Gaps** (addressed):
- Streaming architecture: Tee the stream approach
- Memory schema: Full schema with conversation ID and memory types
- Injection format: XML-tagged blocks
- API scope: Locked to OpenAI-compatible for v1
- Failure mode: Fail-open defined

---

## Work Objectives

### Core Objective
Build a production-ready Rust daemon that automatically enriches LLM conversations with relevant memories, providing organic-feeling context continuity across sessions.

### Concrete Deliverables
- `nova-memory` binary (daemon mode)
- `nova-cli` binary (management commands)
- Configuration file format (TOML)
- LanceDB storage with three-tier architecture
- Vector search with semantic similarity
- Memory weight system with decay/reinforcement
- SSE streaming proxy for OpenAI-compatible APIs

### Definition of Done
- [ ] `nova-memory serve` starts daemon, proxies requests to upstream LLM
- [ ] Memories automatically injected into system prompts
- [ ] Memories automatically extracted from assistant responses
- [ ] Storage tiers work (hot/warm/cold migration)
- [ ] Eviction works under storage pressure
- [ ] All tests pass: `cargo test`
- [ ] Daemon runs continuously without memory leaks for 24h

### Must Have
- SSE streaming passthrough with zero perceptible latency
- Semantic search returning relevant memories
- Weight-based retention and decay
- Configurable storage limits
- Fail-open error handling
- CLI for basic management (list, delete, stats)

### Must NOT Have (Guardrails)
- NO API key storage (pass-through only)
- NO web UI (CLI/config only)
- NO memory editing (delete-only)
- NO non-text content (images, files)
- NO Anthropic/Google API formats (v1)
- NO memory deduplication/merging (v1)
- NO buffering entire response before forwarding
- Token injection budget default ≤ 2000 tokens

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: NO (greenfield)
- **User wants tests**: YES (after implementation)
- **Framework**: `cargo test` with standard Rust testing

### Test Approach
Each phase includes test tasks AFTER implementation tasks. Tests verify:
1. Unit behavior (functions, structs)
2. Integration behavior (component interactions)
3. End-to-end behavior (full proxy flow)

### Automated Verification
All acceptance criteria use executable commands:
- **API verification**: `curl` commands with JSON assertions
- **Daemon verification**: `pty_spawn` for long-running process testing
- **Storage verification**: Direct LanceDB queries
- **Performance verification**: `time` command with latency thresholds

---

## Execution Strategy

### Phase Overview

```
Phase 0: Project Setup & Skeleton (Wave 0)
├── Initialize Cargo workspace
├── Set up directory structure
├── Configure dependencies
└── Create basic CLI skeleton

Phase 1: Core Storage + Embeddings (Waves 1-2)
├── Memory schema and types
├── LanceDB integration
├── e5-small embedding pipeline
├── Basic CRUD operations
└── Persistence verification

Phase 2: Router + Ingestion (Waves 3-4)
├── DistilBERT-NER integration
├── Entity extraction pipeline
├── Weight calculation system
├── Memory ingestion from text
└── Decay/reinforcement logic

Phase 3: Daemon Proxy (Waves 5-6)
├── HTTP proxy server (axum/hyper)
├── SSE streaming passthrough
├── Request interception + injection
├── Response capture + ingestion
└── Fail-open error handling

Phase 4: Capacity Management (Waves 7-8)
├── Storage tier migration
├── Compaction logic
├── Eviction under pressure
├── Tombstone creation
└── CLI management commands
```

### Critical Path
```
0.1 (cargo init) → 1.1 (schema) → 1.3 (LanceDB) → 1.5 (embeddings) 
→ 2.1 (router) → 2.3 (ingestion) → 3.1 (proxy server) → 3.3 (injection)
→ 4.1 (tiers) → 4.3 (eviction)
```

---

## TODOs

### Phase 0: Project Setup & Skeleton

- [x] 0.1. Initialize Cargo Workspace

  **What to do**:
  - Run `cargo init --name nova-memory`
  - Create workspace structure with `nova-memory` (daemon) and `nova-cli` (management) crates
  - Add initial Cargo.toml with workspace configuration
  - Set up rust-analyzer configuration

  **Must NOT do**:
  - Don't add all dependencies yet (add incrementally per phase)
  - Don't create complex directory structure yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple scaffolding task, single command + basic file edits
  - **Skills**: []
    - No specialized skills needed for cargo init

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (first task)
  - **Blocks**: All subsequent tasks
  - **Blocked By**: None

  **References**:
  - Cargo workspace docs: https://doc.rust-lang.org/cargo/reference/workspaces.html

  **Acceptance Criteria**:
  ```bash
  # Verify workspace exists
  cargo check
  # Assert: Exit code 0
  
  # Verify structure
  ls -la
  # Assert: Contains Cargo.toml, src/
  
  cat Cargo.toml | grep "name"
  # Assert: Contains "nova-memory"
  ```

  **Commit**: YES
  - Message: `chore: initialize nova-memory cargo workspace`
  - Files: `Cargo.toml`, `src/main.rs`

---

- [x] 0.2. Configure Core Dependencies

  **What to do**:
  - Add tokio (async runtime)
  - Add serde + serde_json (serialization)
  - Add toml (config parsing)
  - Add tracing + tracing-subscriber (logging)
  - Add thiserror (error handling)
  - Add uuid (identifiers)
  - Add chrono (timestamps)
  - Configure feature flags appropriately

  **Must NOT do**:
  - Don't add LanceDB, fastembed, candle yet (Phase 1)
  - Don't add HTTP framework yet (Phase 3)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Cargo.toml editing, simple task
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 0.3, 0.4
  - **Blocked By**: 0.1

  **References**:
  - tokio: https://docs.rs/tokio
  - serde: https://serde.rs/

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0, no missing dependencies
  
  grep "tokio" Cargo.toml
  # Assert: tokio dependency present
  ```

  **Commit**: YES
  - Message: `chore: add core dependencies (tokio, serde, tracing)`
  - Files: `Cargo.toml`

---

- [x] 0.3. Create Module Structure

  **What to do**:
  - Create src/lib.rs for shared library code
  - Create module directories:
    - `src/config/` - Configuration loading
    - `src/memory/` - Memory types and operations
    - `src/storage/` - LanceDB and tier management
    - `src/router/` - Memory router (DistilBERT)
    - `src/embedding/` - e5-small embeddings
    - `src/proxy/` - HTTP proxy and streaming
    - `src/cli/` - CLI commands
  - Create mod.rs files with placeholder exports
  - Create error.rs with custom error types

  **Must NOT do**:
  - Don't implement functionality yet
  - Don't add implementation-specific imports

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: File/directory creation, no logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 0 (with 0.4)
  - **Blocks**: Phase 1 tasks
  - **Blocked By**: 0.2

  **References**:
  - Rust module system: https://doc.rust-lang.org/book/ch07-02-defining-modules-to-control-scope-and-privacy.html

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0
  
  ls src/
  # Assert: Contains config, memory, storage, router, embedding, proxy, cli directories
  
  cat src/lib.rs
  # Assert: Contains pub mod declarations
  ```

  **Commit**: YES
  - Message: `chore: create module structure skeleton`
  - Files: `src/**/*.rs`

---

- [x] 0.4. Create Configuration Schema

  **What to do**:
  - Define `Config` struct in `src/config/mod.rs`
  - Include all configurable options:
    ```rust
    pub struct Config {
        pub storage: StorageConfig,
        pub proxy: ProxyConfig,
        pub router: RouterConfig,
        pub embedding: EmbeddingConfig,
    }
    
    pub struct StorageConfig {
        pub hot_cache_gb: u64,      // default: 10
        pub warm_storage_gb: u64,   // default: 50
        pub cold_enabled: bool,     // default: true
        pub data_dir: PathBuf,      // default: ~/.nova-memory
    }
    
    pub struct ProxyConfig {
        pub listen_addr: String,    // default: 127.0.0.1:9999
        pub upstream_url: String,   // required
        pub timeout_secs: u64,      // default: 300
        pub max_injection_tokens: usize, // default: 2000
    }
    ```
  - Implement TOML deserialization with serde
  - Create default config file template

  **Must NOT do**:
  - Don't implement config file loading yet (just the types)
  - Don't validate config values yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Type definitions with serde derives
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 0 (with 0.3)
  - **Blocks**: Phase 3 proxy tasks
  - **Blocked By**: 0.2

  **References**:
  - serde TOML: https://docs.rs/toml

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0
  
  cargo test config
  # Assert: Config struct compiles, default values work
  ```

  **Commit**: YES
  - Message: `feat(config): define configuration schema`
  - Files: `src/config/mod.rs`

---

### Phase 1: Core Storage + Embeddings

- [x] 1.1. Define Memory Schema

  **What to do**:
  - Create `Memory` struct in `src/memory/types.rs`:
    ```rust
    pub struct Memory {
        pub id: Uuid,
        pub content: String,
        pub embedding: Vec<f32>,
        pub memory_type: MemoryType,
        pub weight: f32,
        pub created_at: DateTime<Utc>,
        pub last_accessed: DateTime<Utc>,
        pub access_count: u32,
        pub conversation_id: Option<String>,
        pub entities: Vec<String>,
        pub source: MemorySource,
        pub tier: StorageTier,
        pub compression: CompressionLevel,
    }
    
    pub enum MemoryType {
        Episodic,   // What happened (conversations, events)
        Semantic,   // Facts and knowledge
        Procedural, // How to do things
    }
    
    pub enum MemorySource {
        Conversation,
        File,
        Web,
        Manual,
    }
    
    pub enum StorageTier {
        Hot,
        Warm,
        Cold,
    }
    
    pub enum CompressionLevel {
        Full,
        Summary,
        Keywords,
        Hash,
    }
    ```
  - Implement serde Serialize/Deserialize
  - Implement Arrow schema conversion for LanceDB

  **Must NOT do**:
  - Don't implement memory operations yet
  - Don't connect to LanceDB yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Type definitions, derives
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (first in Phase 1)
  - **Blocks**: 1.2, 1.3, 1.4, 1.5
  - **Blocked By**: 0.3

  **References**:
  - Arrow Rust: https://docs.rs/arrow
  - LanceDB schema: https://lancedb.github.io/lancedb/basic/

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0
  
  cargo test memory::types
  # Assert: Memory struct serializes/deserializes correctly
  ```

  **Commit**: YES
  - Message: `feat(memory): define memory schema and types`
  - Files: `src/memory/types.rs`, `src/memory/mod.rs`

---

- [x] 1.2. Define Tombstone Schema

  **What to do**:
  - Create `Tombstone` struct in `src/memory/tombstone.rs`:
    ```rust
    pub struct Tombstone {
        pub original_id: Uuid,
        pub evicted_at: DateTime<Utc>,
        pub topics: Vec<String>,
        pub participants: Vec<String>,
        pub approximate_date: DateTime<Utc>,
        pub reason: EvictionReason,
    }
    
    pub enum EvictionReason {
        StoragePressure,
        LowWeight,
        Superseded { by: Uuid },
        ManualDeletion,
    }
    ```
  - Implement display formatting for "I forgot X but..." messages

  **Must NOT do**:
  - Don't implement eviction logic yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Type definitions
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with 1.3)
  - **Blocks**: 4.4 (tombstone creation)
  - **Blocked By**: 1.1

  **References**:
  - Draft spec: `.sisyphus/drafts/nova-memory-system.md`

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0
  
  cargo test tombstone
  # Assert: Tombstone displays correctly
  ```

  **Commit**: YES
  - Message: `feat(memory): define tombstone schema`
  - Files: `src/memory/tombstone.rs`

---

- [x] 1.3. Integrate LanceDB

  **What to do**:
  - Add `lancedb = "0.23"` to Cargo.toml
  - Create `src/storage/lance.rs`:
    - `LanceStore` struct wrapping LanceDB connection
    - `connect(path: &Path)` - connect to/create database
    - `create_table(name: &str, schema: &Schema)` - create memory table
    - Arrow schema for Memory struct
  - Create memories table with vector column
  - Configure IVF-PQ index for vector search

  **Must NOT do**:
  - Don't implement CRUD operations yet (task 1.4)
  - Don't implement tiering yet (Phase 4)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: External library integration, schema mapping
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with 1.2)
  - **Blocks**: 1.4, 1.5
  - **Blocked By**: 1.1

  **References**:
  - LanceDB Rust: https://lancedb.github.io/lancedb/
  - LanceDB crate: https://docs.rs/lancedb
  - Arrow schema: https://docs.rs/arrow-schema

  **Acceptance Criteria**:
  ```bash
  cargo check
  # Assert: Exit code 0
  
  cargo test storage::lance
  # Assert: Can connect to LanceDB, create table
  ```

  **Commit**: YES
  - Message: `feat(storage): integrate LanceDB with memory schema`
  - Files: `Cargo.toml`, `src/storage/lance.rs`, `src/storage/mod.rs`

---

- [x] 1.4. Implement Memory CRUD Operations

  **What to do**:
  - Extend `LanceStore` with CRUD methods:
    - `insert(memory: &Memory)` - insert single memory
    - `insert_batch(memories: &[Memory])` - batch insert
    - `get(id: Uuid)` - get by ID
    - `delete(id: Uuid)` - delete by ID
    - `update_access(id: Uuid)` - increment access count, update last_accessed
  - Handle embedding storage as FixedSizeList
  - Implement proper error handling with thiserror

  **Must NOT do**:
  - Don't implement vector search yet (task 1.6)
  - Don't implement weight updates yet (Phase 2)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Database operations, async code
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 1.6
  - **Blocked By**: 1.3

  **References**:
  - LanceDB CRUD: https://lancedb.github.io/lancedb/basic/
  - tokio async: https://tokio.rs/tokio/tutorial

  **Acceptance Criteria**:
  ```bash
  cargo test storage::lance::crud
  # Assert: Insert, get, delete all work
  # Assert: Inserted memory can be retrieved
  # Assert: Deleted memory returns None
  ```

  **Commit**: YES
  - Message: `feat(storage): implement memory CRUD operations`
  - Files: `src/storage/lance.rs`

---

- [x] 1.5. Integrate e5-small Embeddings

  **What to do**:
  - Add `fastembed = "5.8"` to Cargo.toml
  - Create `src/embedding/mod.rs`:
    - `EmbeddingModel` struct wrapping fastembed
    - `new()` - initialize with e5-small model
    - `embed(text: &str) -> Vec<f32>` - single text embedding
    - `embed_batch(texts: &[&str]) -> Vec<Vec<f32>>` - batch embedding
  - Model downloads on first use (cache in data dir)
  - Embedding dimension: 384

  **Must NOT do**:
  - Don't implement caching yet
  - Don't implement quantization yet

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: ML model integration, external crate
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with 1.6)
  - **Blocks**: 1.6, 2.3
  - **Blocked By**: 1.1

  **References**:
  - fastembed-rs: https://docs.rs/fastembed
  - e5-small model: https://huggingface.co/intfloat/e5-small-v2

  **Acceptance Criteria**:
  ```bash
  cargo test embedding
  # Assert: Model loads successfully
  # Assert: embed("hello world") returns Vec<f32> with length 384
  # Assert: Latency < 100ms for single embed
  ```

  **Commit**: YES
  - Message: `feat(embedding): integrate e5-small via fastembed`
  - Files: `Cargo.toml`, `src/embedding/mod.rs`

---

- [x] 1.6. Implement Vector Search

  **What to do**:
  - Extend `LanceStore` with search methods:
    - `search(embedding: &[f32], limit: usize) -> Vec<Memory>` - basic similarity search
    - `search_filtered(embedding: &[f32], filter: &MemoryFilter, limit: usize)` - filtered search
  - Create `MemoryFilter` struct:
    ```rust
    pub struct MemoryFilter {
        pub memory_types: Option<Vec<MemoryType>>,
        pub min_weight: Option<f32>,
        pub since: Option<DateTime<Utc>>,
        pub conversation_id: Option<String>,
    }
    ```
  - Use LanceDB's ANN search with distance threshold
  - Return memories sorted by similarity

  **Must NOT do**:
  - Don't implement weight-based reranking yet (Phase 2)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Vector search, filtering logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with 1.5)
  - **Blocks**: 2.4
  - **Blocked By**: 1.4

  **References**:
  - LanceDB vector search: https://lancedb.github.io/lancedb/basic/#search
  - LanceDB filtering: https://lancedb.github.io/lancedb/sql/

  **Acceptance Criteria**:
  ```bash
  cargo test storage::lance::search
  # Assert: Search returns similar memories
  # Assert: Filter by memory_type works
  # Assert: Search latency < 50ms for 10K memories
  ```

  **Commit**: YES
  - Message: `feat(storage): implement vector similarity search`
  - Files: `src/storage/lance.rs`

---

- [x] 1.7. Add Phase 1 Tests

  **What to do**:
  - Create `tests/storage_tests.rs`:
    - Test memory insertion and retrieval
    - Test vector search accuracy
    - Test persistence across restarts
  - Create `tests/embedding_tests.rs`:
    - Test embedding generation
    - Test embedding similarity (same content = high similarity)
  - Add test fixtures for consistent testing

  **Must NOT do**:
  - Don't test router (Phase 2)
  - Don't test proxy (Phase 3)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: Standard test writing
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (end of Phase 1)
  - **Blocks**: Phase 2
  - **Blocked By**: 1.6

  **References**:
  - Rust testing: https://doc.rust-lang.org/book/ch11-01-writing-tests.html

  **Acceptance Criteria**:
  ```bash
  cargo test
  # Assert: All Phase 1 tests pass
  # Assert: Test count >= 10
  ```

  **Commit**: YES
  - Message: `test: add Phase 1 storage and embedding tests`
  - Files: `tests/storage_tests.rs`, `tests/embedding_tests.rs`

---

### Phase 2: Router + Ingestion Pipeline

- [x] 2.1. Integrate DistilBERT-NER

  **What to do**:
  - Add `candle-core`, `candle-transformers`, `candle-nn` to Cargo.toml
  - Create `src/router/ner.rs`:
    - `NerModel` struct wrapping DistilBERT
    - `new()` - load model from HuggingFace Hub
    - `extract_entities(text: &str) -> Vec<Entity>` - extract named entities
  - Define `Entity` struct:
    ```rust
    pub struct Entity {
        pub text: String,
        pub label: EntityLabel,  // Person, Organization, Location, etc.
        pub confidence: f32,
    }
    ```
  - Handle model caching in data directory

  **Must NOT do**:
  - Don't implement topic extraction yet (task 2.2)
  - Don't implement Qwen fallback yet (post-v1)

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
    - Reason: ML model integration, Candle framework complexity
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (first in Phase 2)
  - **Blocks**: 2.2, 2.3
  - **Blocked By**: 1.7

  **References**:
  - Candle: https://github.com/huggingface/candle
  - DistilBERT-NER: https://huggingface.co/distilbert-base-cased
  - Candle examples: https://github.com/huggingface/candle/tree/main/candle-examples

  **Acceptance Criteria**:
  ```bash
  cargo test router::ner
  # Assert: Model loads successfully
  # Assert: extract_entities("I work at Acme Corp with Sarah") 
  #         returns entities containing "Acme Corp" (ORG) and "Sarah" (PER)
  # Assert: Latency < 50ms
  ```

  **Commit**: YES
  - Message: `feat(router): integrate DistilBERT-NER for entity extraction`
  - Files: `Cargo.toml`, `src/router/ner.rs`, `src/router/mod.rs`

---

- [x] 2.2. Implement Router Output

  **What to do**:
  - Create `src/router/mod.rs` with `MemoryRouter` struct:
    ```rust
    pub struct RouterOutput {
        pub topics: Vec<String>,
        pub entities: Vec<Entity>,
        pub emotional_valence: f32,  // -1.0 to 1.0 (simplified)
        pub query_keys: Vec<String>, // Terms for memory lookup
        pub search_types: Vec<MemoryType>,
    }
    ```
  - Implement `route(text: &str) -> RouterOutput`:
    - Call NER model for entities
    - Extract topics from entities + noun phrases
    - Simple sentiment heuristic (positive/negative keywords)
    - Generate query keys from significant terms

  **Must NOT do**:
  - Don't implement complex sentiment analysis (use heuristic)
  - Don't implement urgency detection

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Combines multiple extraction steps
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 2.4
  - **Blocked By**: 2.1

  **References**:
  - Task 2.1 NER output

  **Acceptance Criteria**:
  ```bash
  cargo test router
  # Assert: route("I love working with Sarah at Acme Corp") returns:
  #         - entities: ["Sarah", "Acme Corp"]
  #         - emotional_valence > 0 (positive due to "love")
  #         - query_keys contains "Sarah", "Acme Corp"
  ```

  **Commit**: YES
  - Message: `feat(router): implement memory router with topic/entity extraction`
  - Files: `src/router/mod.rs`

---

- [x] 2.3. Implement Memory Ingestion

  **What to do**:
  - Create `src/memory/ingestion.rs`:
    - `IngestionPipeline` struct
    - `ingest(text: &str, source: MemorySource, conversation_id: Option<String>) -> Memory`:
      1. Run router to get entities/topics
      2. Generate embedding with e5-small
      3. Calculate initial weight
      4. Determine memory type (Episodic for conversations)
      5. Create Memory struct
      6. Store in LanceDB
  - Implement filtering rules:
    - Skip empty/whitespace-only content
    - Skip very short content (< 10 chars)
    - Determine compression level based on content length

  **Must NOT do**:
  - Don't implement deduplication
  - Don't ingest user messages (assistant only)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Combines router, embedding, storage
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 3.4
  - **Blocked By**: 2.2, 1.5

  **References**:
  - Draft spec weight factors section

  **Acceptance Criteria**:
  ```bash
  cargo test memory::ingestion
  # Assert: ingest("User prefers dark mode", Conversation, Some("conv-123"))
  #         creates Memory with:
  #         - embedding.len() == 384
  #         - memory_type == Episodic
  #         - conversation_id == Some("conv-123")
  #         - weight > 0
  ```

  **Commit**: YES
  - Message: `feat(memory): implement ingestion pipeline`
  - Files: `src/memory/ingestion.rs`

---

- [x] 2.4. Implement Weight Calculation

  **What to do**:
  - Create `src/memory/weight.rs`:
    - `calculate_initial_weight(router_output: &RouterOutput, source: MemorySource) -> f32`
    - `calculate_effective_weight(memory: &Memory, config: &WeightConfig) -> f32`
  - Weight factors from spec:
    ```rust
    pub struct WeightConfig {
        pub access_multiplier: f32,     // default: 0.1
        pub decay_rate: f32,            // default: 0.1 per day
        pub emotional_multiplier: f32,  // default: 0.3
        pub owner_multiplier: f32,      // default: 0.5
        pub association_multiplier: f32, // default: 0.05
    }
    ```
  - Effective weight formula:
    ```
    base * ln(access_count) * exp(-decay_rate * age_days) 
    * (1 + emotional * emotional_mult) * (1 + owner_importance * owner_mult)
    ```

  **Must NOT do**:
  - Don't implement association network (v2)
  - Don't implement explicit importance marking yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Math formulas, straightforward logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with 2.5)
  - **Blocks**: 2.6, 4.3
  - **Blocked By**: 2.2

  **References**:
  - Draft spec weight factors section

  **Acceptance Criteria**:
  ```bash
  cargo test memory::weight
  # Assert: Initial weight > 0 for valid input
  # Assert: Effective weight decreases over time (decay)
  # Assert: Effective weight increases with access count
  # Assert: Emotional content has higher weight
  ```

  **Commit**: YES
  - Message: `feat(memory): implement weight calculation system`
  - Files: `src/memory/weight.rs`

---

- [x] 2.5. Implement Memory Retrieval with Reranking

  **What to do**:
  - Create `src/memory/retrieval.rs`:
    - `retrieve(router_output: &RouterOutput, limit: usize) -> Vec<RetrievedMemory>`
    - `RetrievedMemory` struct with memory + scores:
      ```rust
      pub struct RetrievedMemory {
          pub memory: Memory,
          pub similarity_score: f32,
          pub effective_weight: f32,
          pub final_score: f32,  // Combined ranking
      }
      ```
  - Two-stage retrieval:
    1. Vector search for top 3*limit candidates
    2. Rerank by effective weight
    3. Return top limit
  - Update access stats on retrieved memories

  **Must NOT do**:
  - Don't retrieve tombstones yet (Phase 4)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multi-stage retrieval, scoring
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with 2.4)
  - **Blocks**: 3.3
  - **Blocked By**: 1.6, 2.2

  **References**:
  - Task 1.6 vector search
  - Task 2.4 weight calculation

  **Acceptance Criteria**:
  ```bash
  cargo test memory::retrieval
  # Assert: retrieve returns memories sorted by final_score
  # Assert: Higher-weight memories rank higher at equal similarity
  # Assert: access_count incremented on retrieved memories
  ```

  **Commit**: YES
  - Message: `feat(memory): implement retrieval with weight-based reranking`
  - Files: `src/memory/retrieval.rs`

---

- [x] 2.6. Add Phase 2 Tests

  **What to do**:
  - Create `tests/router_tests.rs`:
    - Test entity extraction
    - Test router output completeness
  - Create `tests/ingestion_tests.rs`:
    - Test ingestion pipeline end-to-end
    - Test filtering rules
  - Create `tests/weight_tests.rs`:
    - Test decay over time
    - Test access reinforcement

  **Must NOT do**:
  - Don't test proxy (Phase 3)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: Standard test writing
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (end of Phase 2)
  - **Blocks**: Phase 3
  - **Blocked By**: 2.5

  **References**:
  - Phase 2 implementation tasks

  **Acceptance Criteria**:
  ```bash
  cargo test
  # Assert: All Phase 2 tests pass
  # Assert: Test count >= 20
  ```

  **Commit**: YES
  - Message: `test: add Phase 2 router and ingestion tests`
  - Files: `tests/router_tests.rs`, `tests/ingestion_tests.rs`, `tests/weight_tests.rs`

---

### Phase 3: Daemon Proxy

- [x] 3.1. Create HTTP Server Skeleton

  **What to do**:
  - Add `axum`, `hyper`, `tower` to Cargo.toml
  - Create `src/proxy/server.rs`:
    - `ProxyServer` struct with configuration
    - `serve(config: &ProxyConfig)` - start HTTP server
    - Basic health endpoint: `GET /health`
    - Catch-all route for proxying
  - Configure graceful shutdown with tokio signals

  **Must NOT do**:
  - Don't implement actual proxying yet (task 3.2)
  - Don't implement memory injection yet (task 3.3)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: HTTP server setup, axum framework
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (first in Phase 3)
  - **Blocks**: 3.2, 3.3, 3.4
  - **Blocked By**: 2.6

  **References**:
  - axum: https://docs.rs/axum
  - axum examples: https://github.com/tokio-rs/axum/tree/main/examples

  **Acceptance Criteria**:
  ```bash
  # Start server in background
  cargo run -- serve --config test-config.toml &
  sleep 2
  
  curl http://localhost:9999/health
  # Assert: Returns 200 OK
  
  # Cleanup
  kill %1
  ```

  **Commit**: YES
  - Message: `feat(proxy): create HTTP server skeleton with axum`
  - Files: `Cargo.toml`, `src/proxy/server.rs`, `src/proxy/mod.rs`

---

- [x] 3.2. Implement SSE Streaming Passthrough

  **What to do**:
  - Create `src/proxy/streaming.rs`:
    - `StreamingProxy` for handling SSE responses
    - Tee the stream: forward to client while buffering for ingestion
    - Parse SSE events (`data:`, `[DONE]`)
    - Handle chunked transfer encoding
  - Use `futures::stream` for async streaming
  - Buffer complete response for post-stream ingestion

  **Must NOT do**:
  - Don't parse response content yet (task 3.4)
  - Don't add any latency to the stream

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
    - Reason: Complex async streaming, SSE parsing
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 3.3
  - **Blocked By**: 3.1

  **References**:
  - SSE spec: https://html.spec.whatwg.org/multipage/server-sent-events.html
  - hyper streaming: https://hyper.rs/guides/1/server/echo/
  - tokio streams: https://tokio.rs/tokio/tutorial/streams

  **Acceptance Criteria**:
  ```bash
  # Start proxy pointing to mock SSE server
  cargo run -- serve --upstream http://mock-sse:8080 &
  sleep 2
  
  # Send streaming request through proxy
  curl -N http://localhost:9999/v1/chat/completions \
    -H "Content-Type: application/json" \
    -d '{"stream": true, ...}' | head -5
  # Assert: SSE chunks received in real-time (not buffered)
  # Assert: Each chunk matches upstream timing
  ```

  **Commit**: YES
  - Message: `feat(proxy): implement SSE streaming passthrough with tee`
  - Files: `src/proxy/streaming.rs`

---

- [x] 3.3. Implement Request Interception + Memory Injection

  **What to do**:
  - Create `src/proxy/injection.rs`:
    - `inject_memories(request: &mut Request, memories: &[RetrievedMemory])`
    - Parse OpenAI-format request body
    - Find/create system message
    - Append XML-tagged memory block:
      ```
      <nova-memories>
      <memory timestamp="2024-01-15" type="episodic">
        User prefers dark mode for all applications.
      </memory>
      ...
      </nova-memories>
      ```
    - Respect token budget (count tokens, truncate if needed)
  - Integrate with retrieval pipeline:
    1. Parse incoming user message
    2. Run router
    3. Retrieve relevant memories
    4. Inject into system prompt
    5. Forward to upstream

  **Must NOT do**:
  - Don't implement token counting (use char approximation: chars/4)
  - Don't modify user messages

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: JSON manipulation, request modification
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 3.4
  - **Blocked By**: 3.2, 2.5

  **References**:
  - OpenAI chat format: https://platform.openai.com/docs/api-reference/chat/create
  - Task 2.5 retrieval

  **Acceptance Criteria**:
  ```bash
  # Insert test memory
  cargo run -- memory add "User's favorite color is blue"
  
  # Send request through proxy (with logging enabled)
  curl http://localhost:9999/v1/chat/completions \
    -H "Content-Type: application/json" \
    -d '{"messages": [{"role": "user", "content": "What color do I like?"}]}'
  
  # Check logs for injected system prompt
  # Assert: Logs show <nova-memories> block added
  # Assert: Memory about blue color is in the block
  ```

  **Commit**: YES
  - Message: `feat(proxy): implement memory injection into requests`
  - Files: `src/proxy/injection.rs`

---

- [x] 3.4. Implement Response Capture + Ingestion

  **What to do**:
  - Create `src/proxy/capture.rs`:
    - `capture_response(buffered_response: &str, conversation_id: &str)`
    - Parse assistant message from OpenAI response format
    - Extract content from streamed chunks
    - Call ingestion pipeline with response content
  - Handle streaming completion:
    1. Stream completes
    2. Reconstruct full response from buffer
    3. Parse assistant content
    4. Ingest as Episodic memory
  - Skip ingestion for errors, empty responses

  **Must NOT do**:
  - Don't ingest function calls/tool use (text only)
  - Don't block on ingestion (fire and forget)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Response parsing, async ingestion
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 3.5
  - **Blocked By**: 3.3, 2.3

  **References**:
  - OpenAI streaming format: https://platform.openai.com/docs/api-reference/streaming
  - Task 2.3 ingestion

  **Acceptance Criteria**:
  ```bash
  # Send request that will generate response
  curl http://localhost:9999/v1/chat/completions \
    -d '{"messages": [{"role": "user", "content": "Tell me about yourself"}]}'
  
  # Check that response was ingested
  cargo run -- memory list --limit 1
  # Assert: Most recent memory contains assistant's response content
  # Assert: memory_type == Episodic
  # Assert: source == Conversation
  ```

  **Commit**: YES
  - Message: `feat(proxy): implement response capture and ingestion`
  - Files: `src/proxy/capture.rs`

---

- [x] 3.5. Implement Fail-Open Error Handling

  **What to do**:
  - Create `src/proxy/error.rs`:
    - `ProxyError` enum with variants
    - `handle_error(error: ProxyError, request: Request) -> Response`:
      - Log error with tracing
      - Attempt passthrough without memory features
      - If passthrough fails, return 502 Bad Gateway
  - Wrap all memory operations in error handling:
    - Router fails → skip injection, pass through
    - Retrieval fails → skip injection, pass through
    - Ingestion fails → ignore, response already sent
    - Upstream fails → return upstream error

  **Must NOT do**:
  - Don't retry failed operations
  - Don't queue failed ingestions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: Error handling patterns, straightforward
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with 3.6)
  - **Blocks**: None
  - **Blocked By**: 3.4

  **References**:
  - thiserror: https://docs.rs/thiserror
  - axum error handling: https://docs.rs/axum/latest/axum/error_handling/

  **Acceptance Criteria**:
  ```bash
  # Simulate LanceDB failure (corrupt DB path)
  NOVA_DATA_DIR=/nonexistent cargo run -- serve &
  sleep 2
  
  # Request should still work (fail-open)
  curl http://localhost:9999/v1/chat/completions \
    -d '{"messages": [...]}'
  # Assert: Response received (proxied without memory)
  # Assert: Logs show error about LanceDB
  ```

  **Commit**: YES
  - Message: `feat(proxy): implement fail-open error handling`
  - Files: `src/proxy/error.rs`

---

- [x] 3.6. Add Phase 3 Tests

  **What to do**:
  - Create `tests/proxy_tests.rs`:
    - Test health endpoint
    - Test passthrough without memory
    - Test memory injection format
  - Create `tests/streaming_tests.rs`:
    - Test SSE passthrough timing
    - Test response reconstruction
  - Use mock upstream server for testing

  **Must NOT do**:
  - Don't test against real OpenAI API

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Integration tests, mock servers
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with 3.5)
  - **Blocks**: Phase 4
  - **Blocked By**: 3.4

  **References**:
  - axum testing: https://docs.rs/axum/latest/axum/index.html#testing
  - wiremock: https://docs.rs/wiremock

  **Acceptance Criteria**:
  ```bash
  cargo test proxy
  # Assert: All Phase 3 tests pass
  # Assert: Test count >= 10
  ```

  **Commit**: YES
  - Message: `test: add Phase 3 proxy and streaming tests`
  - Files: `tests/proxy_tests.rs`, `tests/streaming_tests.rs`

---

### Phase 4: Capacity Management

- [x] 4.1. Implement Storage Tier Migration

  **What to do**:
  - Create `src/storage/tiers.rs`:
    - `TierManager` struct
    - `migrate(memory_id: Uuid, from: StorageTier, to: StorageTier)`
    - `promote(memory_id: Uuid)` - warm → hot on access
    - `demote(memory_id: Uuid)` - hot → warm, warm → cold
  - Migration triggers:
    - Hot full → demote lowest weight to warm
    - Warm full → demote lowest weight to cold
    - Access count threshold → promote to hotter tier
  - For v1: Hot and Warm are same LanceDB, Cold is separate archive table

  **Must NOT do**:
  - Don't implement external cold storage (S3, etc.)
  - Don't implement async background migration

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Storage management logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (first in Phase 4)
  - **Blocks**: 4.2, 4.3
  - **Blocked By**: 3.6

  **References**:
  - Draft spec storage tiers section

  **Acceptance Criteria**:
  ```bash
  cargo test storage::tiers
  # Assert: migrate() moves memory between tiers
  # Assert: Memory tier field updated correctly
  # Assert: Memory still retrievable after migration
  ```

  **Commit**: YES
  - Message: `feat(storage): implement storage tier migration`
  - Files: `src/storage/tiers.rs`

---

- [x] 4.2. Implement Compaction

  **What to do**:
  - Create `src/storage/compaction.rs`:
    - `Compactor` struct
    - `compact(tier: StorageTier)` - compress old memories
    - Compression strategies:
      - Full → Summary: Generate summary with simple extraction
      - Summary → Keywords: Extract key terms only
      - Keywords → Hash: Keep metadata only
  - Compaction triggers:
    - Memory age > 30 days → Summary
    - Memory age > 90 days → Keywords
    - Configurable thresholds

  **Must NOT do**:
  - Don't use LLM for summarization (simple extraction)
  - Don't compact memories with high weight

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Compression logic, content transformation
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 6 (with 4.3)
  - **Blocks**: 4.5
  - **Blocked By**: 4.1

  **References**:
  - Draft spec compression levels

  **Acceptance Criteria**:
  ```bash
  cargo test storage::compaction
  # Assert: compact() reduces content size
  # Assert: compression_level field updated
  # Assert: Embedding preserved (still searchable)
  ```

  **Commit**: YES
  - Message: `feat(storage): implement memory compaction`
  - Files: `src/storage/compaction.rs`

---

- [x] 4.3. Implement Eviction

  **What to do**:
  - Create `src/storage/eviction.rs`:
    - `Evictor` struct
    - `evict_if_needed(tier: StorageTier) -> Vec<Uuid>` - evict lowest priority
    - `eviction_priority(memory: &Memory) -> f32` - calculate priority score
  - Priority formula (lower = evict first):
    ```
    effective_weight + recency_bonus + association_bonus
    ```
  - Eviction thresholds:
    - Warning at 70% capacity
    - Start eviction at 80% capacity
    - Aggressive eviction at 95% capacity

  **Must NOT do**:
  - Don't evict memories with owner_importance > 0
  - Don't evict memories accessed in last 24h

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Priority calculation, capacity monitoring
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 6 (with 4.2)
  - **Blocks**: 4.4
  - **Blocked By**: 4.1, 2.4

  **References**:
  - Draft spec eviction order
  - Task 2.4 weight calculation

  **Acceptance Criteria**:
  ```bash
  cargo test storage::eviction
  # Assert: Eviction triggered at capacity threshold
  # Assert: Lowest priority memories evicted first
  # Assert: Protected memories not evicted
  ```

  **Commit**: YES
  - Message: `feat(storage): implement capacity-based eviction`
  - Files: `src/storage/eviction.rs`

---

- [x] 4.4. Implement Tombstone Creation

  **What to do**:
  - Extend eviction to create tombstones:
    - Before deleting memory, extract tombstone data
    - Store tombstone in separate table
    - Include in retrieval results when relevant
  - Tombstone display in memory injection:
    ```xml
    <nova-tombstone timestamp="2024-01-15" topics="project-x, alice">
      I previously knew details about this topic but no longer have them.
    </nova-tombstone>
    ```

  **Must NOT do**:
  - Don't store full content in tombstone
  - Don't create tombstones for manual deletions (user privacy)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: Extends existing eviction logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 4.5
  - **Blocked By**: 4.3, 1.2

  **References**:
  - Task 1.2 tombstone schema

  **Acceptance Criteria**:
  ```bash
  cargo test storage::tombstone
  # Assert: Eviction creates tombstone
  # Assert: Tombstone contains topics, participants, date
  # Assert: Tombstone searchable by topic
  ```

  **Commit**: YES
  - Message: `feat(storage): create tombstones on eviction`
  - Files: `src/storage/eviction.rs`, `src/storage/tombstone.rs`

---

- [x] 4.5. Implement CLI Management Commands

  **What to do**:
  - Create `src/cli/mod.rs` with commands:
    - `nova-cli memory list [--limit N] [--type TYPE]` - list memories
    - `nova-cli memory show <ID>` - show memory details
    - `nova-cli memory delete <ID>` - delete memory (no tombstone)
    - `nova-cli memory add <TEXT>` - manually add memory
    - `nova-cli stats` - show storage statistics
    - `nova-cli compact [--tier TIER]` - trigger compaction
    - `nova-cli config show` - show current config
  - Use clap for argument parsing
  - Output in table format (human) or JSON (--json flag)

  **Must NOT do**:
  - Don't implement memory editing
  - Don't implement import/export

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: CLI framework, multiple commands
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: 4.6
  - **Blocked By**: 4.4

  **References**:
  - clap: https://docs.rs/clap
  - comfy-table: https://docs.rs/comfy-table

  **Acceptance Criteria**:
  ```bash
  # Test list
  cargo run -- memory list --limit 5
  # Assert: Shows table with ID, content preview, weight, tier
  
  # Test add
  cargo run -- memory add "Test memory content"
  # Assert: Returns new memory ID
  
  # Test stats
  cargo run -- stats
  # Assert: Shows storage usage per tier
  ```

  **Commit**: YES
  - Message: `feat(cli): implement management commands`
  - Files: `src/cli/mod.rs`, `src/cli/memory.rs`, `src/cli/stats.rs`

---

- [x] 4.6. Add Phase 4 Tests

  **What to do**:
  - Create `tests/capacity_tests.rs`:
    - Test tier migration
    - Test compaction
    - Test eviction under pressure
    - Test tombstone creation
  - Create `tests/cli_tests.rs`:
    - Test CLI commands
    - Test JSON output format

  **Must NOT do**:
  - N/A

  **Recommended Agent Profile**:
  - **Category**: `unspecified-low`
    - Reason: Standard test writing
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (end of Phase 4)
  - **Blocks**: None
  - **Blocked By**: 4.5

  **References**:
  - Phase 4 implementation tasks

  **Acceptance Criteria**:
  ```bash
  cargo test
  # Assert: All tests pass
  # Assert: Test count >= 40
  ```

  **Commit**: YES
  - Message: `test: add Phase 4 capacity and CLI tests`
  - Files: `tests/capacity_tests.rs`, `tests/cli_tests.rs`

---

- [ ] 4.7. Final Integration Test

  **What to do**:
  - Create `tests/integration_test.rs`:
    - Full end-to-end test of proxy flow
    - Start daemon
    - Send multiple requests through proxy
    - Verify memory injection working
    - Verify ingestion working
    - Verify capacity management working
  - Create test script for manual verification

  **Must NOT do**:
  - Don't test against production APIs

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex integration test
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (final task)
  - **Blocks**: None
  - **Blocked By**: 4.6

  **References**:
  - All previous tasks

  **Acceptance Criteria**:
  ```bash
  cargo test integration
  # Assert: Full flow works end-to-end
  
  # Manual verification script
  ./scripts/test-full-flow.sh
  # Assert: 10 requests processed
  # Assert: Memories created and retrieved
  # Assert: No errors in logs
  ```

  **Commit**: YES
  - Message: `test: add final integration test`
  - Files: `tests/integration_test.rs`, `scripts/test-full-flow.sh`

---

## Commit Strategy

| Phase | After Task | Message | Key Files |
|-------|------------|---------|-----------|
| 0 | 0.1 | `chore: initialize cargo workspace` | Cargo.toml |
| 0 | 0.4 | `feat(config): define configuration schema` | src/config/ |
| 1 | 1.3 | `feat(storage): integrate LanceDB` | src/storage/ |
| 1 | 1.5 | `feat(embedding): integrate e5-small` | src/embedding/ |
| 1 | 1.7 | `test: add Phase 1 tests` | tests/ |
| 2 | 2.1 | `feat(router): integrate DistilBERT-NER` | src/router/ |
| 2 | 2.3 | `feat(memory): implement ingestion` | src/memory/ |
| 2 | 2.6 | `test: add Phase 2 tests` | tests/ |
| 3 | 3.2 | `feat(proxy): implement SSE streaming` | src/proxy/ |
| 3 | 3.4 | `feat(proxy): implement full proxy flow` | src/proxy/ |
| 3 | 3.6 | `test: add Phase 3 tests` | tests/ |
| 4 | 4.3 | `feat(storage): implement capacity management` | src/storage/ |
| 4 | 4.5 | `feat(cli): implement management commands` | src/cli/ |
| 4 | 4.7 | `test: add final integration test` | tests/ |

---

## Success Criteria

### Verification Commands
```bash
# Build succeeds
cargo build --release
# Expected: Exit code 0, binary at target/release/nova-memory

# All tests pass
cargo test
# Expected: Exit code 0, 40+ tests pass

# Daemon starts and proxies
./target/release/nova-memory serve --config config.toml &
curl http://localhost:9999/health
# Expected: 200 OK

# Memory flow works
curl -X POST http://localhost:9999/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]}'
# Expected: Response received, memory ingested

# CLI works
./target/release/nova-cli memory list
# Expected: Shows ingested memory
```

### Final Checklist
- [ ] All "Must Have" features implemented
- [ ] All "Must NOT Have" guardrails respected
- [ ] All tests pass (`cargo test`)
- [ ] Daemon runs without memory leaks (24h test)
- [ ] SSE streaming has zero perceptible latency
- [ ] Fail-open works (memory errors don't block requests)
- [ ] Configuration file documented
