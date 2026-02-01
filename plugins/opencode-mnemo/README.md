# opencode-mnemo

OpenCode plugin for [Mnemo](https://github.com/watzon/mnemo) - an LLM memory proxy that injects semantic memories into your AI conversations.

## What it does

- **Health check on startup** - Warns if Mnemo isn't running when you start a session
- **Configuration guidance** - Shows how to configure your provider to use Mnemo
- **Status logging** - Debug logging for integration troubleshooting

## Installation

### From npm

Add to your `opencode.json`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["opencode-mnemo"]
}
```

### From local files

Copy `src/index.ts` to `.opencode/plugins/mnemo.ts` or `~/.config/opencode/plugins/mnemo.ts`.

## Configuration

This plugin checks if Mnemo is running, but **you must configure your provider** to route through Mnemo.

### Provider Setup

Add the Mnemo proxy URL to your provider's `baseURL` in `opencode.json`:

#### Anthropic

```json
{
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.anthropic.com/v1"
      }
    }
  }
}
```

#### OpenAI

```json
{
  "provider": {
    "openai": {
      "options": {
        "baseURL": "http://localhost:9999/p/https://api.openai.com/v1"
      }
    }
  }
}
```

### Custom Mnemo Host/Port

If Mnemo runs on a different host or port, set environment variables:

```bash
export MNEMO_HOST=192.168.1.100
export MNEMO_PORT=8080
```

Then update your provider baseURL accordingly:

```json
{
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://192.168.1.100:8080/p/https://api.anthropic.com/v1"
      }
    }
  }
}
```

## Starting Mnemo

Before using this plugin, start the Mnemo daemon:

```bash
# If installed
mnemo

# From source
cargo run --bin mnemo

# With debug logging
RUST_LOG=debug cargo run --bin mnemo
```

Mnemo runs at `http://localhost:9999` by default.

## How Mnemo Works

Mnemo is an HTTP proxy that:

1. Intercepts LLM API calls
2. Retrieves relevant memories from its vector database
3. Injects them into the conversation context
4. Forwards the enriched request to the actual API

Your conversations gain persistent memory across sessions, enabling the AI to recall previous discussions, preferences, and context.

## Troubleshooting

### "Mnemo is not running" warning

1. Start Mnemo: `cargo run --bin mnemo`
2. Verify it's running: `curl http://localhost:9999/health`
3. Check the port isn't blocked by a firewall

### Provider not using Mnemo

1. Verify `baseURL` is set correctly in `opencode.json`
2. Check the URL format: `http://localhost:9999/p/{original-api-url}`
3. Restart OpenCode after config changes

### Debug logging

Enable debug output:

```bash
RUST_LOG=debug mnemo
```

## API

The plugin exports utilities for advanced usage:

```typescript
import { checkMnemoHealth, getMnemoProxyUrl, getProviderConfigs, MNEMO_DEFAULTS } from "opencode-mnemo";

// Check if Mnemo is running
const health = await checkMnemoHealth("localhost", 9999);
console.log(health.running, health.version);

// Generate proxy URL
const url = getMnemoProxyUrl("https://api.anthropic.com/v1");
// => "http://localhost:9999/p/https://api.anthropic.com/v1"

// Get pre-configured provider settings
const configs = getProviderConfigs();
// => { anthropic: { baseURL: "..." }, openai: { baseURL: "..." } }
```

## License

MIT
