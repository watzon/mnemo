# @mnemo/openclaw

OpenClaw plugin for Mnemo memory proxy integration. Routes LLM API requests through Mnemo for automatic semantic memory injection.

## Installation

```bash
openclaw plugins install @mnemo/openclaw
```

Or link locally for development:

```bash
openclaw plugins install -l ./plugins/openclaw-mnemo
```

## Prerequisites

Mnemo must be running at the configured URL (default: `http://localhost:9999`).

```bash
# Start Mnemo
cargo run --bin mnemo
```

## Configuration

Add to your OpenClaw config:

```yaml
plugins:
  entries:
    mnemo:
      enabled: true
      config:
        url: "http://localhost:9999"
        enabled: true
        healthCheckInterval: 30000
        providers:
          openai: true
          anthropic: true

gateway:
  providers:
    openai:
      baseUrl: "http://localhost:9999/p/https://api.openai.com/v1"
    anthropic:
      baseUrl: "http://localhost:9999/p/https://api.anthropic.com/v1"
```

## CLI Commands

### Check Status

```bash
openclaw mnemo status
```

Shows:
- Mnemo URL and enabled state
- Connection health
- Provider routing configuration
- Configured passthrough URLs

### Configuration Helper

```bash
openclaw mnemo configure
openclaw mnemo configure --url http://custom:8080
```

Displays configuration instructions for routing providers through Mnemo.

## RPC Methods

### `mnemo.status`

Returns current configuration and health status.

```json
{
  "config": {
    "url": "http://localhost:9999",
    "enabled": true,
    "providers": { "openai": true, "anthropic": true }
  },
  "health": {
    "available": true,
    "lastCheck": "2026-02-01T00:00:00.000Z"
  },
  "providerUrls": {
    "openai": "http://localhost:9999/p/https://api.openai.com/v1",
    "anthropic": "http://localhost:9999/p/https://api.anthropic.com/v1"
  }
}
```

### `mnemo.health`

Performs a live health check against Mnemo.

```json
{
  "available": true,
  "url": "http://localhost:9999",
  "lastCheck": "2026-02-01T00:00:00.000Z"
}
```

## Background Service

The plugin runs a background health monitor that:
- Checks Mnemo availability at the configured interval (default: 30 seconds)
- Logs state changes (available/unavailable)
- Maintains health status for RPC queries

## How It Works

Mnemo is an HTTP proxy that intercepts LLM API requests and:

1. Retrieves relevant memories based on the conversation context
2. Injects memories into the system prompt
3. Forwards the request to the upstream provider
4. Captures responses for future recall

By configuring OpenClaw providers to use Mnemo passthrough URLs (`/p/{upstream-url}`), all LLM requests automatically benefit from semantic memory injection.

## Development

```bash
cd plugins/openclaw-mnemo
npm install
npm run build
```
