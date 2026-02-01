# Mnemo Documentation

This directory contains guides for integrating Mnemo with various AI tools and applications.

## What is Mnemo?

Mnemo is a transparent HTTP proxy that gives your LLM long-term memory. It sits between your AI tools and LLM APIs (OpenAI, Anthropic, etc.), automatically:

- **Injecting relevant memories** into system prompts
- **Capturing responses** for future recall
- **Performing semantic search** to retrieve contextually relevant information

All without modifying your existing workflow.

## How It Works

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│   Your AI Tool   │────▶│      Mnemo       │────▶│    LLM API       │
│  (OpenClaw,      │◀────│  (Proxy Daemon)  │◀────│  (OpenAI/        │
│   OpenCode, etc) │     │  localhost:9999  │     │   Anthropic)     │
└──────────────────┘     └──────────────────┘     └──────────────────┘
                                │
                                ▼
                         ┌──────────────────┐
                         │   Memory Store   │
                         │    (LanceDB)     │
                         └──────────────────┘
```

## Integration Guides

| Tool | Description | Guide |
|------|-------------|-------|
| **OpenClaw** | Self-hosted AI assistant for messaging platforms | [integrations/openclaw.md](integrations/openclaw.md) |
| **OpenCode** | Open-source terminal AI coding assistant | [integrations/opencode.md](integrations/opencode.md) |

## Official Plugins

For easier integration, we provide official plugins:

| Plugin | Package | Features |
|--------|---------|----------|
| **OpenCode** | `opencode-mnemo` | Health checks, config guidance, status logging |
| **OpenClaw** | `@mnemo/openclaw` | CLI commands, background monitoring, RPC methods |

Plugins are in the `plugins/` directory and can be published to npm.

## Quick Start

### 1. Install Mnemo

```bash
# From source
git clone https://github.com/watzon/mnemo.git
cd mnemo
cargo build --release
cp target/release/mnemo /usr/local/bin/
cp target/release/mnemo-cli /usr/local/bin/

# Or using cargo
cargo install mnemo mnemo-cli
```

### 2. Configure Mnemo

Create `~/.mnemo/config.toml`:

```toml
[storage]
data_dir = "~/.mnemo"

[proxy]
listen_addr = "127.0.0.1:9999"
timeout_secs = 300
max_injection_tokens = 2000
allowed_hosts = ["api.openai.com", "api.anthropic.com"]

[router]
strategy = "semantic"
max_memories = 10
relevance_threshold = 0.7
```

### 3. Start the Daemon

```bash
mnemo
```

### 4. Configure Your Tool

Point your AI tool to `http://localhost:9999` instead of the direct API URL. See the integration guides above for tool-specific instructions.

## Key Concepts

### Dynamic Passthrough

Mnemo supports routing to any LLM provider via the `/p/{url}` endpoint:

```bash
# Route to OpenAI
http://localhost:9999/p/https://api.openai.com/v1/chat/completions

# Route to Anthropic
http://localhost:9999/p/https://api.anthropic.com/v1/messages
```

This means you can use Mnemo with any tool that supports custom API endpoints, even if it doesn't have explicit "proxy" support.

### Memory Injection

Memories are injected as structured XML in system prompts:

```xml
<mnemo-memories>
<memory timestamp="2024-01-15" type="semantic">
  User prefers dark mode for all applications.
</memory>
<memory timestamp="2024-01-14" type="episodic">
  User is learning Rust and asks detailed questions about ownership.
</memory>
</mnemo-memories>
```

### Provider Auto-Detection

Mnemo automatically detects whether requests are for OpenAI or Anthropic based on:
1. URL patterns (`*.openai.com`, `*.anthropic.com`)
2. Headers (`x-api-key` vs `Authorization: Bearer`)
3. Request body structure

No additional configuration needed.

## Managing Memories

Use the `mnemo-cli` tool to inspect and manage memories:

```bash
# List recent memories
mnemo-cli memory list

# Add a memory manually
mnemo-cli memory add "User prefers TypeScript over JavaScript" --type semantic

# View statistics
mnemo-cli stats
```

## Troubleshooting

### Mnemo not injecting memories

1. Check that Mnemo is running: `curl http://localhost:9999/health`
2. Verify memories exist: `mnemo-cli memory list`
3. Check logs: `RUST_LOG=debug mnemo`

### Connection refused

1. Ensure Mnemo is listening on the expected address
2. Check firewall rules if accessing from another machine
3. Verify `allowed_hosts` includes your target API

### High latency

1. First request may be slow due to model loading (~5-10s)
2. Subsequent requests should be fast
3. Check `max_injection_tokens` isn't too high

## Further Reading

- [Main README](../README.md) - Full feature list and configuration options
- [config.example.toml](../config.example.toml) - Annotated configuration template
