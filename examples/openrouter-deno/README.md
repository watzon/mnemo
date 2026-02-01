# Mnemo + OpenRouter Example

This example demonstrates using mnemo with OpenRouter as the upstream LLM provider via Deno and the OpenAI SDK.

## Architecture

```
┌─────────────────────┐
│   Deno Client       │
│   (OpenAI SDK)      │
└──────────┬──────────┘
           │ HTTP POST to
           │ http://localhost:9999/p/https://openrouter.ai/api/v1/chat/completions
           ▼
┌─────────────────────┐
│   Mnemo       │
│   (Rust Proxy)      │
│   - Extracts query  │
│   - Searches memories│
│   - Injects context │
└──────────┬──────────┘
           │ Forwarded request with
           │ injected <nova-memories>
           ▼
┌─────────────────────┐
│   OpenRouter API    │
│   (Multi-provider)  │
└─────────────────────┘
```

## Prerequisites

1. **Build mnemo** (from project root):
   ```bash
   cargo build --release
   ```

2. **Get an OpenRouter API key**:
   - Sign up at https://openrouter.ai
   - Create an API key at https://openrouter.ai/keys

3. **Install Deno** (if not already installed):
   ```bash
   curl -fsSL https://deno.land/install.sh | sh
   ```

## Quick Start

### Terminal 1: Start Mnemo

```bash
cd examples/openrouter-deno

# Start the proxy daemon with test config
RUST_LOG=info ../../target/release/mnemo --config config.toml
```

You should see output like:
```
INFO mnemo::proxy::server: Starting Mnemo proxy on 127.0.0.1:9999
INFO mnemo::proxy::server: Proxy ready to accept connections
```

### Terminal 2: Run Tests

```bash
cd examples/openrouter-deno

# Run the test suite
OPENROUTER_API_KEY=sk-or-... deno run --allow-net --allow-env test-chat.ts
```

## Test Scripts

### `test-chat.ts`

Main test suite that validates:
1. **Basic Chat** - Simple completion request
2. **Streaming** - SSE streaming responses
3. **Multi-turn** - Conversation with context
4. **Different Models** - Testing Anthropic via OpenRouter

### `add-memory.ts`

Adds sample memories to test injection:
```bash
deno run --allow-run add-memory.ts
```

Then re-run `test-chat.ts` to see memories injected into requests.

## Configuration

`config.toml` is configured for testing:
- Small storage limits (1GB hot, 5GB warm)
- Cold storage disabled
- Lower relevance threshold (0.5) for easier testing
- OpenRouter in allowed hosts

## Verifying Memory Injection

To see what's being injected, run mnemo with debug logging:

```bash
RUST_LOG=debug ../../target/release/mnemo --config config.toml
```

Look for log lines containing `Injecting memories` or `<nova-memories>`.

## Troubleshooting

### "Connection refused"

Nova-memory isn't running. Start it first.

### "Host not allowed"

The upstream URL isn't in `allowed_hosts`. Check `config.toml`.

### "401 Unauthorized"

Your OpenRouter API key is invalid or missing credits.

### "Model not found"

OpenRouter model format is `provider/model`, e.g.:
- `openai/gpt-4o-mini`
- `anthropic/claude-3-haiku`
- `google/gemini-pro`

See https://openrouter.ai/models for available models.

## Direct cURL Testing

You can also test directly with curl:

```bash
curl http://localhost:9999/p/https://openrouter.ai/api/v1/chat/completions \
  -H "Authorization: Bearer $OPENROUTER_API_KEY" \
  -H "Content-Type: application/json" \
  -H "HTTP-Referer: https://github.com/watzon/mnemo" \
  -d '{
    "model": "openai/gpt-4o-mini",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 50
  }'
```
