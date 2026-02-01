# Using Mnemo with OpenCode

[OpenCode](https://opencode.ai) is an open-source terminal AI coding assistant, often called the open-source alternative to Claude Code. By integrating Mnemo with OpenCode, your coding assistant gains persistent memory of your codebase patterns, preferences, and past interactions.

## Quick Start with Plugin

The easiest way to integrate Mnemo with OpenCode is using the official plugin:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["opencode-mnemo"],
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.anthropic.com/v1"
      }
    }
  }
}
```

The plugin provides:
- **Health checks** on session start - warns if Mnemo isn't running
- **Configuration guidance** - shows how to set up providers
- **Status logging** - debug logging for troubleshooting

## Why Use Mnemo with OpenCode?

- **Project Context**: Remember architectural decisions, coding conventions, and past refactoring discussions
- **Personal Preferences**: Retain your coding style preferences across sessions
- **Cross-Session Memory**: Context from yesterday's debugging session informs today's work
- **Multi-Project Awareness**: Memories can span multiple projects (or be isolated per project)

## Prerequisites

- OpenCode installed ([installation guide](https://opencode.ai/docs/))
- Mnemo installed and running (see [Quick Start](../README.md#quick-start))
- Working API keys for your LLM provider

## Configuration

### Step 1: Configure Mnemo

Create or edit `~/.mnemo/config.toml`:

```toml
[proxy]
listen_addr = "127.0.0.1:9999"
timeout_secs = 300
max_injection_tokens = 2000
allowed_hosts = [
    "api.openai.com",
    "api.anthropic.com",
    "*.groq.com",
]

[router]
strategy = "semantic"
max_memories = 10
relevance_threshold = 0.7
```

Start Mnemo:

```bash
mnemo
```

### Step 2: Configure OpenCode

OpenCode supports custom providers via its JSON configuration. You can configure it globally or per-project.

#### Option A: Global Configuration

Create or edit `~/.config/opencode/opencode.json`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.anthropic.com/v1"
      }
    },
    "openai": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.openai.com/v1"
      }
    }
  }
}
```

#### Option B: Project-Specific Configuration

Create `opencode.json` in your project root:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.anthropic.com/v1"
      }
    }
  }
}
```

Project configs override global configs, so you can have Mnemo-enabled projects alongside direct-API projects.

#### Option C: Using Environment Variables

For quick testing or CI environments:

```bash
# Set the base URL via environment
export OPENCODE_CONFIG_CONTENT='{"provider":{"anthropic":{"options":{"baseURL":"http://localhost:9999/p/https://api.anthropic.com/v1"}}}}'

# Run OpenCode
opencode
```

### Step 3: Verify the Integration

1. Start OpenCode in a project:
   ```bash
   cd your-project
   opencode
   ```

2. Send a message and check Mnemo logs:
   ```bash
   RUST_LOG=debug mnemo
   ```

3. Verify memories are being stored:
   ```bash
   mnemo-cli memory list
   ```

## How It Works

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   OpenCode   │────▶│    Mnemo     │────▶│   Anthropic/ │
│   Terminal   │◀────│    Proxy     │◀────│    OpenAI    │
└──────────────┘     └──────────────┘     └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   LanceDB    │
                     │  (Memories)  │
                     └──────────────┘
```

1. You ask OpenCode a question about your code
2. OpenCode sends the request to Mnemo (thinking it's the LLM API)
3. Mnemo retrieves relevant memories from past sessions
4. Memories are injected into the system prompt
5. Request is forwarded to the actual LLM
6. Response is captured as a new memory
7. Response flows back to OpenCode

## Advanced Configuration

### Using with OpenCode Zen

If you use OpenCode Zen (OpenCode's hosted model service), you can still use Mnemo:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "opencode": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.opencode.ai/v1"
      }
    }
  }
}
```

Add `api.opencode.ai` to Mnemo's allowed hosts:

```toml
[proxy]
allowed_hosts = [
    "api.openai.com",
    "api.anthropic.com",
    "api.opencode.ai",
]
```

### Using with LiteLLM

If you're using LiteLLM as a model router, chain it with Mnemo:

```
OpenCode → Mnemo → LiteLLM → Various LLM APIs
```

Configure OpenCode to point to Mnemo, and Mnemo to point to LiteLLM:

```json
{
  "provider": {
    "litellm": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "LiteLLM via Mnemo",
      "options": {
        "baseURL": "http://localhost:9999/p/http://localhost:4000/v1"
      },
      "models": {
        "gpt-4": { "name": "GPT-4" },
        "claude-3-5-sonnet": { "name": "Claude 3.5 Sonnet" }
      }
    }
  }
}
```

Add LiteLLM to allowed hosts:

```toml
[proxy]
allowed_hosts = [
    "localhost:4000",
    "127.0.0.1:4000",
]
```

### Optimizing for Prompt Caching

Enable deterministic memory retrieval for better cache hit rates with Anthropic:

```toml
[router.deterministic]
enabled = true
decimal_places = 2
topic_overlap_weight = 0.1
```

### Per-Project Memory Isolation

If you want separate memory stores per project, run multiple Mnemo instances:

```bash
# Project A
mnemo --config ~/.mnemo/project-a.toml

# Project B  
mnemo --config ~/.mnemo/project-b.toml
```

Each config can specify a different `data_dir`:

```toml
# ~/.mnemo/project-a.toml
[storage]
data_dir = "~/.mnemo/projects/project-a"

[proxy]
listen_addr = "127.0.0.1:9999"
```

```toml
# ~/.mnemo/project-b.toml
[storage]
data_dir = "~/.mnemo/projects/project-b"

[proxy]
listen_addr = "127.0.0.1:9998"
```

## Memory Management for Coding

### Seeding Project-Specific Memories

Pre-populate memories about your project:

```bash
# Architecture decisions
mnemo-cli memory add "This project uses a hexagonal architecture with ports and adapters" --type semantic
mnemo-cli memory add "All database access goes through the repository pattern" --type semantic

# Coding conventions
mnemo-cli memory add "Use Result<T, Error> for all fallible operations, never panic" --type procedural
mnemo-cli memory add "Prefer composition over inheritance" --type semantic

# Project-specific knowledge
mnemo-cli memory add "The User model is in src/models/user.rs and uses UUID primary keys" --type semantic
```

### Useful Memory Types for Coding

| Type | Use Case | Example |
|------|----------|---------|
| **Semantic** | Facts, conventions, architecture | "We use PostgreSQL with sqlx" |
| **Procedural** | How-to, workflows, commands | "To run tests: cargo test --workspace" |
| **Episodic** | Past decisions, debugging sessions | "Fixed the auth bug by adding token refresh" |

### Viewing Recent Coding Memories

```bash
# See all recent memories
mnemo-cli memory list --limit 20

# Filter by type
mnemo-cli memory list --type procedural
```

## Troubleshooting

### OpenCode not connecting

1. Verify Mnemo is running:
   ```bash
   curl http://localhost:9999/health
   ```

2. Check OpenCode config is valid:
   ```bash
   cat ~/.config/opencode/opencode.json | jq .
   ```

3. Verify the provider baseURL is correct

### "Connection refused" errors

1. Ensure Mnemo is running on the expected port
2. Check if another process is using port 9999:
   ```bash
   lsof -i :9999
   ```

### Memories not appearing in responses

1. Lower the relevance threshold:
   ```toml
   [router]
   relevance_threshold = 0.5
   ```

2. Check that memories exist:
   ```bash
   mnemo-cli memory list
   ```

3. Verify memories are being retrieved (check logs):
   ```bash
   RUST_LOG=debug mnemo 2>&1 | grep -i "retriev"
   ```

### Slow first request

The first request loads ML models (~5-10 seconds). Subsequent requests are fast. This is normal behavior.

### High token usage

If memory injection is using too many tokens:

```toml
[proxy]
max_injection_tokens = 1000  # Reduce from default 2000

[router]
max_memories = 5  # Reduce from default 10
```

## Example Session

After integration, OpenCode will have context from previous sessions:

```
You: How should I handle errors in this function?

OpenCode: Based on our previous discussions about this codebase, you prefer 
using the Result<T, Error> pattern with thiserror for error types. Looking 
at your existing code in src/error.rs, I see you've established a pattern 
of domain-specific error enums.

For this function, I'd suggest:
...
```

The assistant remembers your preferences and past architectural decisions, providing more contextually relevant suggestions.

## Tips for Best Results

1. **Be explicit about preferences**: When you tell OpenCode "I prefer X over Y", Mnemo captures this as a semantic memory

2. **Summarize decisions**: After making architectural decisions, summarize them: "We decided to use X because Y"

3. **Periodic cleanup**: Run `mnemo-cli compact` periodically to optimize storage

4. **Review memories**: Occasionally check what's being remembered:
   ```bash
   mnemo-cli memory list --limit 50
   ```

5. **Delete incorrect memories**: If something wrong was captured:
   ```bash
   mnemo-cli memory delete <UUID>
   ```
