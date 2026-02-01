# Mnemo

<p align="center">
  <img src="media/mnemo-banner.webp" alt="Mnemo" width="600">
</p>

[![Build Status](https://img.shields.io/github/actions/workflow/status/watzon/mnemo/ci.yml)](https://github.com/watzon/mnemo/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

> A transparent HTTP proxy that gives your LLM long-term memory

Mnemo is a Rust daemon that sits between your chat client and LLM APIs (OpenAI and Anthropic), automatically injecting relevant memories into system prompts and capturing assistant responses for future recall. It uses semantic search with local embeddings to retrieve contextually relevant information without modifying your existing workflow.

## Features

- **Transparent Proxy**: Drop-in replacement for LLM API endpoints - no client changes needed
- **Multi-Provider Support**: Native support for OpenAI and Anthropic style APIs with automatic provider detection. Route to any provider via `/p/{url}` dynamic passthrough.
- **Automatic Provider Detection**: Intelligently detects whether requests are for OpenAI or Anthropic based on URL, headers, or request body structure
- **Automatic Memory Injection**: Relevant memories are injected into system prompts as structured XML
- **Response Capture**: Assistant responses are automatically stored as episodic memories
- **Semantic Search**: Uses e5-small embeddings via fastembed for efficient local vector search
- **Three-Tier Storage**: Hot (memory), Warm (disk), and Cold (archive) tiers with automatic migration
- **Weight-Based Retention**: Memories decay over time but important ones persist longer
- **Entity Extraction**: Uses DistilBERT-NER to extract and index entities for better retrieval
- **Streaming Support**: Full support for streaming responses from upstream LLMs
- **CLI Management**: Command-line tool for memory inspection, compaction, and configuration

## Integration Guides

Mnemo works with any tool that supports custom API endpoints. See the [docs](docs/) for detailed integration guides:

| Tool | Description | Guide |
|------|-------------|-------|
| **OpenClaw** | Self-hosted AI assistant for messaging platforms | [docs/integrations/openclaw.md](docs/integrations/openclaw.md) |
| **OpenCode** | Open-source terminal AI coding assistant | [docs/integrations/opencode.md](docs/integrations/opencode.md) |

Official plugins are available in `plugins/` for easier integration.

## Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────┐
│   Client    │────▶│   Mnemo    │────▶│     │  LLM API    │
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
         ┌────────┐  ┌────────┐  ┌─────────┐
         │  Hot   │  │  Warm  │  │  Cold   │
         │(Memory)│  │ (Disk) │  │(Archive)│
         └────────┘  └────────┘  └─────────┘
```

### Memory Types

Mnemo classifies memories into three cognitive categories:

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
git clone https://github.com/watzon/mnemo.git
cd mnemo

# Build release binaries
cargo build --release

# Install binaries
cp target/release/mnemo /usr/local/bin/
cp target/release/mnemo-cli /usr/local/bin/
```

### Using Cargo

```bash
cargo install mnemo mnemo-cli
```

## Configuration

Create a configuration file at `~/.mnemo/config.toml`:

```toml
[storage]
hot_cache_gb = 10
warm_storage_gb = 50
cold_enabled = true
data_dir = "~/.mnemo"

[proxy]
listen_addr = "127.0.0.1:9999"
# upstream_url is optional with dynamic passthrough
# upstream_url = "https://api.openai.com/v1"
timeout_secs = 300
max_injection_tokens = 2000
allowed_hosts = ["api.openai.com", "api.anthropic.com"]

[router]
strategy = "semantic"
max_memories = 10
relevance_threshold = 0.7

[embedding]
provider = "local"
# Model is e5-small with 384 dimensions (configured internally)
dimension = 384
batch_size = 32
```

See `config.example.toml` for all available options.

## Usage

### Starting the Daemon

```bash
# Start with default config location
mnemo

# Start with custom config
mnemo --config /path/to/config.toml
```

### Configuring Your Client

Point your LLM client to the Mnemo proxy:

```bash
# Instead of:
export OPENAI_API_URL="https://api.openai.com/v1"

# Use:
export OPENAI_API_URL="http://127.0.0.1:9999"
```

Most clients (OpenAI SDK, LangChain, etc.) respect the `OPENAI_BASE_URL` or similar environment variables.

### Dynamic Passthrough

Route requests to any LLM provider without changing configuration:

```bash
# OpenAI
curl http://localhost:9999/p/https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]}'

# Anthropic
curl http://localhost:9999/p/https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "claude-3-opus-20240229", "max_tokens": 100, "messages": [{"role": "user", "content": "Hello"}]}'
```

The `allowed_hosts` configuration restricts which upstream hosts can be proxied. Wildcards are supported (e.g., `*.openai.com`). An empty list allows all hosts.

### Provider Detection

Mnemo automatically detects the LLM provider for each request using a priority-based heuristic:

1. **URL Detection**: Requests to `*.openai.com` → OpenAI, `*.anthropic.com` → Anthropic
2. **Header Detection**: `x-api-key` header → Anthropic, `Authorization: Bearer` → OpenAI  
3. **Body Detection**: Top-level `system` field → Anthropic, `messages[].role == "system"` → OpenAI

This means you can use the dynamic passthrough (`/p/{url}`) without any additional configuration - Mnemo will automatically handle the differences between providers.

**Provider-specific behavior:**
- **OpenAI**: Memories injected into the first system message in the `messages` array
- **Anthropic**: Memories injected into the top-level `system` field

If the provider cannot be detected, requests pass through unmodified (fail-open behavior).

### Memory Injection

When you send a request to either OpenAI or Anthropic, Mnemo:

1. Detects the provider from URL, headers, or body structure
2. Extracts the user query from the messages array
3. Performs semantic search for relevant memories
4. Formats matches as XML and injects into the system prompt
5. Forwards the modified request to the upstream LLM

Memory injection is provider-aware:
- **OpenAI**: Appends to the system message in the `messages` array
- **Anthropic**: Appends to the top-level `system` field

**Example injected memory block:**

```xml
<mnemo-memories>
<memory timestamp="2024-01-15" type="episodic">
  User prefers dark mode for all applications.
</memory>
<memory timestamp="2024-01-14" type="semantic">
  User is learning Rust and asks detailed questions about ownership.
</memory>
</mnemo-memories>
```

## CLI Commands

The `mnemo-cli` tool provides memory management capabilities:

### Memory Management

```bash
# List memories (default: 20 most recent)
mnemo-cli memory list

# List with filters
mnemo-cli memory list --limit 50 --type semantic

# Show memory details
mnemo-cli memory show <UUID>

# Delete a memory
mnemo-cli memory delete <UUID>

# Add a manual memory
mnemo-cli memory add "User prefers concise technical explanations" --type semantic
```

### Statistics

```bash
# Show storage statistics
mnemo-cli stats

# Output as JSON
mnemo-cli stats --json
```

### Compaction

```bash
# Compact all storage tiers
mnemo-cli compact

# Compact specific tier only
mnemo-cli compact --tier warm
```

### Configuration

```bash
# Show current configuration
mnemo-cli config show

# Show with custom config file
mnemo-cli config show --config /path/to/config.toml
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
<mnemo-memories>
<memory timestamp="YYYY-MM-DD" type="episodic|semantic|procedural">
  Memory content here...
</memory>
...
</mnemo-memories>
```

### Attributes

- **timestamp**: ISO 8601 date when the memory was created
- **type**: Classification of the memory (episodic, semantic, procedural)

### Token Budget

The `max_injection_tokens` configuration limits how much memory content is injected per request. Memories are sorted by relevance and included until the budget is exhausted.

### LLM Provider Caching

Many LLM providers (notably Anthropic) offer prompt caching that can significantly reduce costs and latency for repeated requests. However, since Mnemo injects different memories based on semantic similarity to each query, the system prompt changes between requests, which can reduce cache hit rates.

To maximize cache efficiency, Mnemo supports **deterministic memory selection**:

```toml
[router.deterministic]
enabled = true          # Enable deterministic ordering
decimal_places = 2      # Score quantization precision
topic_overlap_weight = 0.1  # Boost for entity overlap
```

When enabled, this feature:
- **Quantizes similarity scores** to reduce floating-point variance
- **Boosts memories with entity overlap** for more predictable selection
- **Uses deterministic tie-breaking** (older timestamp → UUID) for consistent ordering

This means queries about the same topic will more reliably retrieve the same memories in the same order, improving cache hit rates while maintaining semantic relevance.

## Development

### Project Structure

```
mnemo/
├── crates/
│   ├── mnemo/      # Core daemon library
│   │   ├── src/
│   │   │   ├── config/   # Configuration management
│   │   │   ├── embedding/# Embedding model interface
│   │   │   ├── memory/   # Memory types and operations
│   │   │   ├── proxy/    # HTTP proxy server
│   │   │   ├── router/   # Request routing and NER
│   │   │   └── storage/  # LanceDB storage layer
│   │   └── Cargo.toml
│   └── mnemo-cli/         # CLI management tool
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
RUST_LOG=debug cargo run --bin mnemo
```

### Testing

```bash
# Run all tests (single-threaded required for ML model tests)
cargo test --workspace -- --test-threads=1

# Run tests for specific crate
cargo test -p mnemo -- --test-threads=1
cargo test -p mnemo-cli

# Run with output
cargo test --workspace -- --test-threads=1 --nocapture
```

**Note:** The `--test-threads=1` flag is required because ML model loading (embeddings, NER) can conflict when tests run in parallel.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Acknowledgments

- [fastembed](https://github.com/Anush008/fastembed-rs) for efficient local embeddings
- [LanceDB](https://lancedb.github.io/lancedb/) for vector storage
- [Candle](https://github.com/huggingface/candle) for ML inference
- [Axum](https://github.com/tokio-rs/axum) for the HTTP server
