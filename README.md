# Nova Memory

[![Build Status](https://img.shields.io/github/actions/workflow/status/watzon/nova-memory/ci.yml)](https://github.com/watzon/nova-memory/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

> A transparent HTTP proxy that gives your LLM long-term memory

Nova Memory is a Rust daemon that sits between your chat client and LLM API (OpenAI-compatible), automatically injecting relevant memories into system prompts and capturing assistant responses for future recall. It uses semantic search with local embeddings to retrieve contextually relevant information without modifying your existing workflow.

## Features

- **Transparent Proxy**: Drop-in replacement for LLM API endpoints - no client changes needed
- **Automatic Memory Injection**: Relevant memories are injected into system prompts as structured XML
- **Response Capture**: Assistant responses are automatically stored as episodic memories
- **Semantic Search**: Uses e5-small embeddings via fastembed for efficient local vector search
- **Three-Tier Storage**: Hot (memory), Warm (disk), and Cold (archive) tiers with automatic migration
- **Weight-Based Retention**: Memories decay over time but important ones persist longer
- **Entity Extraction**: Uses DistilBERT-NER to extract and index entities for better retrieval
- **Streaming Support**: Full support for streaming responses from upstream LLMs
- **CLI Management**: Command-line tool for memory inspection, compaction, and configuration

## Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────┐
│   Client    │────▶│   Nova Memory    │────▶│  LLM API    │
│  (Chat App) │◀────│  (Proxy Daemon)  │◀────│ (OpenAI/    │
└─────────────┘     └──────────────────┘     │ Anthropic)  │
                           │                 └─────────────┘
                           ▼
                    ┌──────────────────┐
                    │   LanceDB        │
                    │  (Vector Store)  │
                    └──────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
         ┌────────┐  ┌────────┐  ┌────────┐
         │  Hot   │  │  Warm  │  │  Cold  │
         │(Memory)│  │ (Disk) │  │(Archive)│
         └────────┘  └────────┘  └────────┘
```

### Memory Types

Nova Memory classifies memories into three cognitive categories:

- **Episodic**: Conversations, events, interactions ("User asked about Rust yesterday")
- **Semantic**: Facts, knowledge, preferences ("User prefers dark mode")
- **Procedural**: How-to instructions, workflows ("To deploy: run cargo build")

### Storage Tiers

Memories migrate between tiers based on access patterns:

| Tier | Location | Access Speed | Retention |
|------|----------|--------------|-----------|
| Hot | In-memory | Fastest | Recently accessed |
| Warm | Local disk (LanceDB) | Fast | Moderately accessed |
| Cold | Archive storage | Slow | Rarely accessed |

## Installation

### Prerequisites

- Rust 1.85 or later
- 4GB RAM minimum (for embedding models)
- ~500MB disk space for models and data

### From Source

```bash
# Clone the repository
git clone https://github.com/watzon/nova-memory.git
cd nova-memory

# Build release binaries
cargo build --release

# Install binaries
cp target/release/nova-memory /usr/local/bin/
cp target/release/nova-cli /usr/local/bin/
```

### Using Cargo

```bash
cargo install nova-memory nova-cli
```

## Configuration

Create a configuration file at `~/.nova-memory/config.toml`:

```toml
[storage]
hot_cache_gb = 10
warm_storage_gb = 50
cold_enabled = true
data_dir = "~/.nova-memory"

[proxy]
listen_addr = "127.0.0.1:9999"
upstream_url = "https://api.openai.com/v1"
timeout_secs = 300
max_injection_tokens = 2000

[router]
strategy = "semantic"
max_memories = 10
relevance_threshold = 0.7

[embedding]
provider = "local"
model = "sentence-transformers/all-MiniLM-L6-v2"
dimension = 384
batch_size = 32
```

See `config.example.toml` for all available options.

## Usage

### Starting the Daemon

```bash
# Start with default config location
nova-memory

# Start with custom config
nova-memory --config /path/to/config.toml
```

### Configuring Your Client

Point your LLM client to the Nova Memory proxy:

```bash
# Instead of:
export OPENAI_API_URL="https://api.openai.com/v1"

# Use:
export OPENAI_API_URL="http://127.0.0.1:9999"
```

Most clients (OpenAI SDK, LangChain, etc.) respect the `OPENAI_BASE_URL` or similar environment variables.

### Memory Injection

When you send a request, Nova Memory:

1. Extracts the user query from the messages array
2. Performs semantic search for relevant memories
3. Formats matches as XML and injects into the system prompt
4. Forwards the modified request to the upstream LLM

**Example injected memory block:**

```xml
<nova-memories>
<memory timestamp="2024-01-15" type="episodic">
  User prefers dark mode for all applications.
</memory>
<memory timestamp="2024-01-14" type="semantic">
  User is learning Rust and asks detailed questions about ownership.
</memory>
</nova-memories>
```

## CLI Commands

The `nova-cli` tool provides memory management capabilities:

### Memory Management

```bash
# List memories (default: 20 most recent)
nova-cli memory list

# List with filters
nova-cli memory list --limit 50 --type semantic

# Show memory details
nova-cli memory show <UUID>

# Delete a memory
nova-cli memory delete <UUID>

# Add a manual memory
nova-cli memory add "User prefers concise technical explanations" --type semantic
```

### Statistics

```bash
# Show storage statistics
nova-cli stats

# Output as JSON
nova-cli stats --json
```

### Compaction

```bash
# Compact all storage tiers
nova-cli compact

# Compact specific tier only
nova-cli compact --tier warm
```

### Configuration

```bash
# Show current configuration
nova-cli config show

# Show with custom config file
nova-cli config show --config /path/to/config.toml
```

### Global Options

All commands support these global flags:

```bash
--json          # Output in JSON format
--data-dir      # Override data directory
--config        # Specify config file path
```

## Memory Injection Format

Memories are injected into system prompts using a structured XML format:

```xml
<nova-memories>
<memory timestamp="YYYY-MM-DD" type="episodic|semantic|procedural">
  Memory content here...
</memory>
...
</nova-memories>
```

### Attributes

- **timestamp**: ISO 8601 date when the memory was created
- **type**: Classification of the memory (episodic, semantic, procedural)

### Token Budget

The `max_injection_tokens` configuration limits how much memory content is injected per request. Memories are sorted by relevance and included until the budget is exhausted.

## Development

### Project Structure

```
nova-memory/
├── crates/
│   ├── nova-memory/      # Core daemon library
│   │   ├── src/
│   │   │   ├── config/   # Configuration management
│   │   │   ├── embedding/# Embedding model interface
│   │   │   ├── memory/   # Memory types and operations
│   │   │   ├── proxy/    # HTTP proxy server
│   │   │   ├── router/   # Request routing and NER
│   │   │   └── storage/  # LanceDB storage layer
│   │   └── Cargo.toml
│   └── nova-cli/         # CLI management tool
│       ├── src/
│       │   └── commands/ # CLI command implementations
│       └── Cargo.toml
├── config.example.toml
├── Cargo.toml
└── README.md
```

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test --workspace

# Run with logging
RUST_LOG=debug cargo run --bin nova-memory
```

### Testing

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p nova-memory
cargo test -p nova-cli

# Run with output
cargo test -- --nocapture
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Acknowledgments

- [fastembed](https://github.com/Anush008/fastembed-rs) for efficient local embeddings
- [LanceDB](https://lancedb.github.io/lancedb/) for vector storage
- [Candle](https://github.com/huggingface/candle) for ML inference
- [Axum](https://github.com/tokio-rs/axum) for the HTTP server
