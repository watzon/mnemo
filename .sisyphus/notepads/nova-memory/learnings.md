
## Task 0.4: Create Configuration Schema

### Serde Configuration Pattern
- Use `#[derive(Deserialize)]` for TOML config parsing
- Implement `Default` trait for all config structs with sensible defaults
- Use `#[serde(default)]` on nested config fields to allow partial TOML files
- Use `#[serde(default = "function_name")]` for custom default values
- Use `PathBuf` for file paths (from `std::path::PathBuf`)

### Default Value Pattern
```rust
#[serde(default = "default_hot_cache_gb")]
pub hot_cache_gb: u64,

fn default_hot_cache_gb() -> u64 {
    10
}
```

### Config Struct Hierarchy
```rust
pub struct Config {
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub proxy: ProxyConfig,
    // ...
}
```

### Testing Config Deserialization
- Test `Default::default()` returns expected values
- Test full TOML deserialization with all fields
- Test partial TOML deserialization (only required fields)
- Use `toml::from_str()` to parse TOML strings in tests

### Default Data Directory Pattern
- Use `dirs::home_dir()` for cross-platform home directory detection
- Chain with `.map()` and `.unwrap_or_else()` for fallback
- Example: `dirs::home_dir().map(|h| h.join(".nova-memory")).unwrap_or_else(|| PathBuf::from(".nova-memory"))`

### Example Config File Structure
- Place `config.example.toml` in project root
- Include comprehensive comments explaining each option
- Group related settings under section headers
- Show default values and valid options
- Mark required fields clearly

### Verification Steps
1. Run `cargo check` to verify compilation
2. Run `cargo test config` to run config-specific tests
3. Verify all 3 tests pass:
   - `test_config_default` - Default values work
   - `test_toml_deserialization` - Full TOML parsing works
   - `test_toml_partial_deserialization` - Partial TOML with defaults works

### Git Commit Strategy
- Split into 2 commits for 4 files:
  1. `feat(config): define configuration schema` - Code + dependencies
  2. `docs: add configuration example file` - Documentation
- Follow conventional commit style from previous commits
- Include detailed commit body explaining changes


## Task 1.1: Define Memory Schema

### Memory Struct Design
- Use `Uuid::new_v4()` for unique identifiers
- Use `DateTime<Utc>` from chrono for timestamps
- Use `Vec<f32>` for embeddings (384 dimensions for e5-small model)
- Use `Vec<String>` for extracted entities
- Include helper methods like `mark_accessed()` and `set_weight()` for common operations
- Use `clamp()` for weight bounds (0.0 to 1.0)

### Enum Design Pattern
- Use `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]` for enums
- Include doc comments explaining each variant
- Group related enums together (MemoryType, MemorySource, StorageTier, CompressionLevel)

### Serde Serialization
- All types derive Serialize/Deserialize for JSON persistence
- Test round-trip serialization for all types
- Enums serialize to their variant names by default

### Test Coverage
- Test struct creation with defaults
- Test serialization/deserialization round-trip
- Test helper methods (mark_accessed, set_weight)
- Test enum serialization for all variants
- Test edge cases (weight clamping)

### Module Structure
- Create `types.rs` for data structures
- Export types in `mod.rs` with `pub use`
- Keep module-level documentation explaining the module's purpose

### Verification Steps
1. Run `cargo check` - should exit with code 0
2. Run `cargo test memory::types` - should pass all 8 tests:
   - test_memory_serialization
   - test_memory_new_defaults
   - test_memory_mark_accessed
   - test_memory_set_weight
   - test_memory_type_serialization
   - test_memory_source_serialization
   - test_storage_tier_serialization
   - test_compression_level_serialization


## Task 1.2: Define Tombstone Schema - Completed

### Implementation Summary
- Created `Tombstone` struct in `crates/nova-memory/src/memory/tombstone.rs`
- Defined `EvictionReason` enum with 4 variants:
  - `StoragePressure` - evicted due to capacity limits
  - `LowWeight` - evicted due to low importance
  - `Superseded { by: Uuid }` - replaced by newer memory
  - `ManualDeletion` - deleted by user/admin
- Implemented `std::fmt::Display` for human-readable messages like:
  "I previously knew about [topics] with [participants] around [date], but this memory was evicted due to [reason]"
- Added comprehensive unit tests covering:
  - Struct creation and field access
  - Serialization/deserialization (serde)
  - Display formatting with various combinations of topics/participants
  - All EvictionReason variants

### Key Design Decisions
- Used Vec<String> for topics and participants to allow flexible metadata preservation
- Display implementation handles empty lists gracefully ("various topics", "unknown participants")
- Multiple items formatted with Oxford comma style ("A, B and C")
- Date formatted as "Month Year" for human readability
- Followed existing patterns from types.rs for consistency

### Technical Notes
- LSP diagnostics confirmed no errors in tombstone.rs
- Pre-existing arrow_array version conflict in lance.rs unrelated to this task
- Repository uses semantic commit style (feat:, docs:, chore:)
- Commit: adb0d9c feat(memory): define tombstone schema


## Task 1.5: Integrate e5-small Embeddings via fastembed - Completed

### Implementation Summary
- Added `fastembed = "5.8"` to workspace Cargo.toml
- Added `fastembed = { workspace = true }` to nova-memory crate Cargo.toml
- Implemented `EmbeddingModel` struct in `crates/nova-memory/src/embedding/mod.rs`:
  - `new()` -> `Result<Self, NovaError>` - initializes fastembed with MultilingualE5Small model
  - `embed(&mut self, text: &str)` -> `Result<Vec<f32>, NovaError>` - single text embedding
  - `embed_batch(&mut self, texts: &[String])` -> `Result<Vec<Vec<f32>>, NovaError>` - batch embedding
- Defined `EMBEDDING_DIMENSION: usize = 384` constant

### Key Technical Details
- `fastembed::TextEmbedding::embed()` requires `&mut self`, so both `embed` and `embed_batch` need mutable references
- Model downloads on first use (~100MB for multilingual-e5-small)
- Using `fastembed::EmbeddingModel::MultilingualE5Small` for the model enum variant

### Testing Notes
- Tests must run serially (`--test-threads=1`) to avoid model loading contention
- Parallel test execution causes "Failed to retrieve onnx/model.onnx" errors due to concurrent model loading attempts
- 4 tests cover:
  - Model loads successfully
  - Embedding dimension is 384
  - Similar texts have higher cosine similarity than different texts
  - Batch embedding works correctly

### Error Handling
- Wrapped fastembed errors using `NovaError::Embedding(e.to_string())`
- Handle missing embedding in single text case with `ok_or_else`

### Pre-existing Issues Noted
- arrow_array version conflict between arrow 55.2.0 and 56.2.0 in storage/lance.rs (unrelated to this task)
- EvictionReason enum now has PartialEq, Eq derives (was fixed separately)

### Verification Commands
- `cargo check -p nova-memory` - exits with code 0
- `cargo test -p nova-memory embedding -- --test-threads=1` - all 4 tests pass

### Commit
- 1226698 feat(embedding): integrate e5-small via fastembed


## Task 1.3: Integrate LanceDB for Vector Storage - Completed

### Implementation Summary
- Created `LanceStore` struct in `crates/nova-memory/src/storage/lance.rs`
- Methods implemented:
  - `connect(path: &Path)` - connects to LanceDB at specified path
  - `create_memories_table()` - creates table with Memory-compatible Arrow schema
  - `open_memories_table()` - opens existing memories table
  - `table_exists(name: &str)` - checks if table exists
  - `create_vector_index()` - creates IVF-PQ index (requires 256+ rows)
  - `memories_table()` - returns optional reference to table

### Arrow Schema Design
- Maps Memory struct fields to Arrow DataTypes:
  - `id` -> Utf8 (string, not nullable)
  - `content` -> Utf8 (string, not nullable)
  - `embedding` -> FixedSizeList(Float32, 384) (for e5-small embeddings)
  - `memory_type`, `source`, `tier`, `compression` -> Utf8 (enum values as strings)
  - `weight` -> Float32
  - `created_at`, `last_accessed` -> Timestamp(Microsecond, "UTC")
  - `access_count` -> Int32
  - `conversation_id` -> Utf8 (nullable)

### LanceDB Rust API Patterns
- Connect: `lancedb::connect(uri).execute().await`
- Create table: `connection.create_table(name, Box::new(batches)).execute().await`
- Create IVF-PQ index: `table.create_index(&["column"], Index::IvfPq(builder)).execute().await`
- IVF-PQ builder: `IvfPqIndexBuilder::default().num_partitions(256).num_sub_vectors(16)`

### Version Compatibility
- LanceDB 0.23.x requires arrow-array/arrow-schema 56.x
- Version mismatch causes "RecordBatchReader trait not implemented" errors
- Always match arrow versions with what lancedb depends on

### IVF-PQ Index Notes
- Requires minimum 256 rows for training
- Common parameters: num_partitions=256, num_sub_vectors=16
- Must check row_count before creating index

### Testing Pattern
- Use `tempfile::tempdir()` for isolated test databases
- Tests are async with `#[tokio::test]`
- Test table creation, opening, and schema correctness

### Dependencies Added to Workspace
- `lancedb = "0.23"`
- `arrow-array = "56"`
- `arrow-schema = "56"`
- `futures = "0.3"`
- `tempfile = "3"` (dev-dependency)

### Commit
- a9343b0 feat(storage): integrate LanceDB with memory schema


## Task 1.4: Implement Memory CRUD Operations - Completed

### Implementation Summary
- Extended `LanceStore` in `crates/nova-memory/src/storage/lance.rs` with CRUD methods:
  - `insert(memory: &Memory)` - insert single memory
  - `insert_batch(memories: &[Memory])` - batch insert
  - `get(id: Uuid)` - retrieve by ID (returns Option<Memory>)
  - `delete(id: Uuid)` - delete by ID (returns bool if found)
  - `update_access(id: Uuid)` - increment access_count and update last_accessed

### LanceDB CRUD API Patterns
- **Insert**: `table.add(Box::new(RecordBatchIterator)).execute().await`
- **Query**: `table.query().only_if("id = 'uuid'").execute().await` returns async stream
- **Delete**: `table.delete("id = 'uuid'").await`
- **Update**: `table.update().only_if("id = 'uuid'").column("col", "expr").execute().await`

### Arrow RecordBatch Conversion
- Convert Memory to RecordBatch:
  - Build arrays for each field (StringArray, Float32Array, etc.)
  - Use `FixedSizeListArray::from_iter_primitive` for embeddings
  - Use `TimestampMicrosecondArray::with_timezone("UTC")` for timestamps
  - Enums stored as strings via match expressions
  
- Convert RecordBatch to Memory:
  - Downcast columns: `batch.column(i).as_any().downcast_ref::<ArrayType>()`
  - Parse UUID from string: `Uuid::parse_str(string)`
  - Parse timestamp: `Utc.timestamp_micros(i64).single().ok_or_else()`
  - Match strings back to enums

### Query Result Handling
- Results come as async stream of RecordBatch
- Use `futures::TryStreamExt::try_collect()` to collect into Vec<RecordBatch>
- Check both batches.is_empty() and batch.num_rows() == 0

### Required Imports
- `futures::TryStreamExt` for stream collection
- `lancedb::query::{ExecutableQuery, QueryBase}` for query builder
- `chrono::TimeZone` for timestamp parsing
- `arrow_array::Array` for downcasting

### Test Coverage (7 tests)
1. `test_insert_and_get` - roundtrip single memory
2. `test_insert_batch` - batch insert multiple memories
3. `test_get_nonexistent_returns_none` - missing ID returns None
4. `test_delete` - delete removes memory
5. `test_delete_nonexistent_returns_false` - missing ID returns false
6. `test_update_access` - increments count and updates timestamp
7. `test_roundtrip_preserves_all_fields` - verifies all field types preserved

### Notes
- Entities field not stored (not in Lance schema yet)
- Delete first checks existence to return correct bool
- Update uses SQL expression `access_count + 1` for atomic increment
