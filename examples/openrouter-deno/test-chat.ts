/**
 * Nova Memory + OpenRouter Test
 *
 * This script tests nova-memory's proxy functionality with OpenRouter.
 * It uses the OpenAI SDK pointing at the nova-memory proxy, which then
 * forwards requests to OpenRouter via dynamic passthrough.
 *
 * Usage:
 *   OPENROUTER_API_KEY=sk-or-... deno run --allow-net --allow-env test-chat.ts
 */

import OpenAI from "npm:openai@4";

// Nova Memory proxy configuration
const NOVA_PROXY = "http://127.0.0.1:9999";
const OPENROUTER_BASE = "https://openrouter.ai/api/v1";

// The full passthrough URL through nova-memory
const PASSTHROUGH_URL = `${NOVA_PROXY}/p/${OPENROUTER_BASE}`;

const apiKey = Deno.env.get("OPENROUTER_API_KEY");
if (!apiKey) {
  console.error("Error: OPENROUTER_API_KEY environment variable is required");
  console.error("Usage: OPENROUTER_API_KEY=sk-or-... deno run --allow-net --allow-env test-chat.ts");
  Deno.exit(1);
}

// Initialize OpenAI client pointing at Nova Memory proxy
const client = new OpenAI({
  apiKey,
  baseURL: PASSTHROUGH_URL,
  defaultHeaders: {
    "HTTP-Referer": "https://github.com/watzon/nova-memory", // Required by OpenRouter
    "X-Title": "Nova Memory Test",
  },
});

console.log("=".repeat(60));
console.log("Nova Memory + OpenRouter Integration Test");
console.log("=".repeat(60));
console.log(`Proxy URL: ${PASSTHROUGH_URL}`);
console.log();

// Test 1: Basic chat completion
async function testBasicChat() {
  console.log("[Test 1] Basic Chat Completion");
  console.log("-".repeat(40));

  try {
    const response = await client.chat.completions.create({
      model: "openai/gpt-4o-mini", // OpenRouter model format
      messages: [
        {
          role: "system",
          content: "You are a helpful assistant. Keep responses brief.",
        },
        {
          role: "user",
          content: "What is nova-memory? Just give a one-sentence answer.",
        },
      ],
      max_tokens: 100,
    });

    console.log("Status: SUCCESS");
    console.log(`Model: ${response.model}`);
    console.log(`Response: ${response.choices[0]?.message?.content}`);
    console.log(`Tokens: ${response.usage?.total_tokens}`);
    return true;
  } catch (error) {
    console.log("Status: FAILED");
    console.error("Error:", error instanceof Error ? error.message : error);
    return false;
  }
}

// Test 2: Streaming response
async function testStreaming() {
  console.log();
  console.log("[Test 2] Streaming Response");
  console.log("-".repeat(40));

  try {
    const stream = await client.chat.completions.create({
      model: "openai/gpt-4o-mini",
      messages: [
        {
          role: "user",
          content: "Count from 1 to 5, with each number on a new line.",
        },
      ],
      max_tokens: 50,
      stream: true,
    });

    process.stdout.write("Response: ");
    for await (const chunk of stream) {
      const content = chunk.choices[0]?.delta?.content;
      if (content) {
        process.stdout.write(content);
      }
    }
    console.log();
    console.log("Status: SUCCESS");
    return true;
  } catch (error) {
    console.log("Status: FAILED");
    console.error("Error:", error instanceof Error ? error.message : error);
    return false;
  }
}

// Test 3: Multi-turn conversation (tests memory injection context)
async function testMultiTurn() {
  console.log();
  console.log("[Test 3] Multi-turn Conversation");
  console.log("-".repeat(40));

  const messages: OpenAI.Chat.ChatCompletionMessageParam[] = [
    {
      role: "system",
      content: "You are a helpful assistant with memory capabilities.",
    },
    {
      role: "user",
      content: "My favorite programming language is Rust. Remember that.",
    },
  ];

  try {
    // First message
    const response1 = await client.chat.completions.create({
      model: "openai/gpt-4o-mini",
      messages,
      max_tokens: 100,
    });

    const assistantReply = response1.choices[0]?.message?.content || "";
    console.log(`User: ${messages[1].content}`);
    console.log(`Assistant: ${assistantReply}`);

    // Add assistant reply and follow-up
    messages.push({ role: "assistant", content: assistantReply });
    messages.push({
      role: "user",
      content: "What's my favorite programming language?",
    });

    // Second message (should recall from conversation context)
    const response2 = await client.chat.completions.create({
      model: "openai/gpt-4o-mini",
      messages,
      max_tokens: 100,
    });

    console.log(`User: ${messages[3].content}`);
    console.log(`Assistant: ${response2.choices[0]?.message?.content}`);
    console.log("Status: SUCCESS");
    return true;
  } catch (error) {
    console.log("Status: FAILED");
    console.error("Error:", error instanceof Error ? error.message : error);
    return false;
  }
}

// Test 4: Different model (Anthropic via OpenRouter)
async function testDifferentModel() {
  console.log();
  console.log("[Test 4] Different Provider (Anthropic via OpenRouter)");
  console.log("-".repeat(40));

  try {
    const response = await client.chat.completions.create({
      model: "anthropic/claude-3-haiku", // Anthropic model via OpenRouter
      messages: [
        {
          role: "user",
          content: "Say 'Hello from Claude!' in exactly 5 words.",
        },
      ],
      max_tokens: 50,
    });

    console.log("Status: SUCCESS");
    console.log(`Model: ${response.model}`);
    console.log(`Response: ${response.choices[0]?.message?.content}`);
    return true;
  } catch (error) {
    console.log("Status: FAILED");
    console.error("Error:", error instanceof Error ? error.message : error);
    return false;
  }
}

// Run all tests
async function main() {
  const results: boolean[] = [];

  results.push(await testBasicChat());
  results.push(await testStreaming());
  results.push(await testMultiTurn());
  results.push(await testDifferentModel());

  console.log();
  console.log("=".repeat(60));
  console.log("Test Summary");
  console.log("=".repeat(60));

  const passed = results.filter(Boolean).length;
  const total = results.length;

  console.log(`Passed: ${passed}/${total}`);
  console.log(`Status: ${passed === total ? "ALL TESTS PASSED" : "SOME TESTS FAILED"}`);

  Deno.exit(passed === total ? 0 : 1);
}

main();
