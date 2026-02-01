# Anthropic Support - Learnings

## 2026-01-31 Task: Initial Setup

### Provider Detection
- Provider enum with 3 variants: OpenAI, Anthropic, Unknown
- Detection priority: URL → Headers → Body structure
- URL: `*.openai.com` → OpenAI, `*.anthropic.com` → Anthropic
- Headers: `x-api-key` → Anthropic, `Authorization: Bearer` → OpenAI
- Body: top-level `system` or `max_tokens` → Anthropic, `messages[].role == "system"` → OpenAI

### LLMProvider Trait
- `inject_memories()` - modifies request body in-place
- `extract_user_query()` - gets last user message content
- `parse_sse_content()` - parses streaming events
- `parse_response_content()` - parses non-streaming JSON

### Anthropic Specifics
- System prompt is top-level `system` field (string), not in messages array
- Content can be string OR array of content blocks
- SSE uses named events: `event: content_block_delta\ndata: {...}`
- Skip `thinking_delta` and `input_json_delta` types
- Response content is array of blocks with `type: "text"`

### OpenAI Specifics
- System prompt is first message with `role: "system"`
- Uses existing `inject_memories()` and `extract_user_query()` functions
- SSE uses `data: {...}` format without named events
- Response content at `choices[0].message.content`

## 2026-01-31 Task: Proxy Memory Injection

### Forward Request Hook
- Inject memories after reading request body bytes, before upstream request
- Fail-open: any injection error logs debug and sends original body
- Use Provider::detect + LLMProvider to extract query and inject memories
- RetrievalPipeline::with_defaults + router_config.max_memories drives lookup

## 2026-01-31 Task: Response Capture Logging

### Provider-Specific Response Parsing
- Added try_capture_response helper to parse non-streaming JSON first, then SSE for streaming
- Provider detection uses original request body + target URL + headers
- Captured content logged at debug level (ingestion deferred)
