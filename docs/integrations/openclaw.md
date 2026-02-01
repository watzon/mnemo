# Using Mnemo with OpenClaw

[OpenClaw](https://openclaw.ai) is a powerful, self-hosted AI personal assistant that connects to messaging platforms like WhatsApp, Telegram, Discord, Slack, and more. By integrating Mnemo with OpenClaw, your assistant gains persistent memory across all conversations and channels.

## Quick Start with Plugin

The easiest way to integrate Mnemo with OpenClaw is using the official plugin:

```bash
# Install the plugin
openclaw plugins install @mnemo/openclaw

# Check status
openclaw mnemo status

# View configuration instructions
openclaw mnemo configure
```

The plugin provides:
- **CLI commands** - `openclaw mnemo status` and `openclaw mnemo configure`
- **Background monitoring** - Periodic health checks with state-change logging
- **RPC methods** - `mnemo.status` and `mnemo.health` for programmatic access

## Why Use Mnemo with OpenClaw?

- **Cross-Channel Memory**: Memories from Telegram conversations can inform responses in WhatsApp
- **Persistent Context**: Your assistant remembers user preferences, past interactions, and learned facts
- **Zero Lock-in**: Memories are stored locally, not in a cloud service
- **Transparent**: No changes to OpenClaw's core functionality

## Prerequisites

- OpenClaw installed and running ([installation guide](https://docs.openclaw.ai/install))
- Mnemo installed and running (see [Quick Start](../README.md#quick-start))
- Working API keys for your LLM provider (OpenAI, Anthropic, etc.)

## Configuration

### Step 1: Configure Mnemo

Ensure Mnemo is configured with appropriate allowed hosts. Edit `~/.mnemo/config.toml`:

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

### Step 2: Configure OpenClaw Gateway

OpenClaw uses a gateway architecture for LLM communication. You need to configure it to route through Mnemo.

#### Option A: Using Environment Variables

Set the base URL for your provider to point to Mnemo's passthrough endpoint:

```bash
# For OpenAI
export OPENAI_BASE_URL="http://localhost:9999/p/https://api.openai.com/v1"

# For Anthropic
export ANTHROPIC_BASE_URL="http://localhost:9999/p/https://api.anthropic.com/v1"
```

Then start OpenClaw:

```bash
claw
```

#### Option B: Using OpenClaw Configuration

Edit your OpenClaw configuration file (typically `~/.claw/config.yaml` or via `claw configure`):

```yaml
gateway:
  providers:
    openai:
      baseUrl: "http://localhost:9999/p/https://api.openai.com/v1"
      apiKey: "${OPENAI_API_KEY}"
    
    anthropic:
      baseUrl: "http://localhost:9999/p/https://api.anthropic.com/v1"
      apiKey: "${ANTHROPIC_API_KEY}"
```

#### Option C: Using the CLI

```bash
claw gateway config set openai.baseUrl "http://localhost:9999/p/https://api.openai.com/v1"
claw gateway config set anthropic.baseUrl "http://localhost:9999/p/https://api.anthropic.com/v1"
```

### Step 3: Verify the Integration

1. Send a message to your OpenClaw assistant via any channel
2. Check Mnemo logs for the request:
   ```bash
   RUST_LOG=debug mnemo
   ```
3. Verify memories are being stored:
   ```bash
   mnemo-cli memory list
   ```

## How It Works

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  WhatsApp/   │────▶│   OpenClaw   │────▶│    Mnemo     │────▶│   OpenAI/    │
│  Telegram/   │◀────│   Gateway    │◀────│    Proxy     │◀────│  Anthropic   │
│  Discord     │     └──────────────┘     └──────────────┘     └──────────────┘
└──────────────┘                                │
                                                ▼
                                         ┌──────────────┐
                                         │   LanceDB    │
                                         │  (Memories)  │
                                         └──────────────┘
```

1. User sends message via messaging platform
2. OpenClaw receives and processes the message
3. OpenClaw's gateway sends the LLM request to Mnemo
4. Mnemo retrieves relevant memories and injects them into the system prompt
5. Request is forwarded to the actual LLM API
6. Response is captured by Mnemo as a new memory
7. Response flows back through OpenClaw to the user

## Advanced Configuration

### Multi-Model Setup

If you use different models for different purposes, you can configure each:

```yaml
gateway:
  providers:
    openai:
      baseUrl: "http://localhost:9999/p/https://api.openai.com/v1"
      models:
        gpt-4o:
          name: "GPT-4o"
        gpt-4o-mini:
          name: "GPT-4o Mini"
    
    anthropic:
      baseUrl: "http://localhost:9999/p/https://api.anthropic.com/v1"
      models:
        claude-sonnet-4-20250514:
          name: "Claude Sonnet"
        claude-3-haiku-20240307:
          name: "Claude Haiku"
```

### Optimizing for Prompt Caching

Anthropic offers prompt caching that can reduce costs. Enable deterministic memory retrieval in Mnemo:

```toml
[router.deterministic]
enabled = true
decimal_places = 2
topic_overlap_weight = 0.1
```

This ensures similar queries retrieve the same memories in the same order, improving cache hit rates.

### Running on a Remote Server

If OpenClaw and Mnemo are on different machines:

1. Configure Mnemo to listen on all interfaces:
   ```toml
   [proxy]
   listen_addr = "0.0.0.0:9999"
   ```

2. Update OpenClaw to use the remote address:
   ```yaml
   gateway:
     providers:
       openai:
         baseUrl: "http://mnemo-server:9999/p/https://api.openai.com/v1"
   ```

3. Consider using HTTPS with a reverse proxy (nginx, caddy) for production

## Memory Management

### Seeding Initial Memories

Pre-populate memories about your users or preferences:

```bash
# Add user preferences
mnemo-cli memory add "User prefers concise responses" --type semantic
mnemo-cli memory add "User's timezone is America/New_York" --type semantic
mnemo-cli memory add "User works in software development" --type semantic

# Add procedural knowledge
mnemo-cli memory add "To check user's calendar: use the calendar tool" --type procedural
```

### Viewing Memories by Type

```bash
# See semantic memories (facts, preferences)
mnemo-cli memory list --type semantic

# See episodic memories (conversations, events)
mnemo-cli memory list --type episodic
```

### Compacting Storage

Over time, memories accumulate. Compact periodically:

```bash
mnemo-cli compact
```

## Troubleshooting

### OpenClaw not connecting through Mnemo

1. Verify Mnemo is running: `curl http://localhost:9999/health`
2. Check the base URL configuration in OpenClaw
3. Ensure `allowed_hosts` includes your LLM provider

### Memories not being injected

1. Check relevance threshold - try lowering it:
   ```toml
   [router]
   relevance_threshold = 0.5
   ```
2. Verify memories exist: `mnemo-cli memory list`
3. Check Mnemo logs for retrieval activity

### Slow responses

1. First request loads ML models (~5-10s) - subsequent requests are fast
2. Reduce `max_memories` if injecting too many memories
3. Consider reducing `max_injection_tokens`

### Memory not capturing responses

Ensure streaming is working correctly. Check logs:

```bash
RUST_LOG=debug mnemo 2>&1 | grep -i "capture\|memory"
```

## Example Interaction

After integration, your OpenClaw assistant will have context from previous conversations:

```
User (via Telegram): What was that Rust library we discussed yesterday?