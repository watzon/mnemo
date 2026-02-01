# Nova Memory Examples

This directory contains working examples demonstrating nova-memory integration with various LLM providers and client libraries.

## Available Examples

| Example | Description | Client |
|---------|-------------|--------|
| [openrouter-deno](./openrouter-deno/) | OpenRouter multi-provider proxy | Deno + OpenAI SDK |

## Prerequisites

All examples require nova-memory to be built:

```bash
cargo build --release
```

## Adding New Examples

Each example should be self-contained with:
- `config.toml` - Nova-memory configuration
- `README.md` - Setup and usage instructions
- Test scripts demonstrating the integration

Data directories (`test-data/`) are gitignored.
