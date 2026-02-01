/**
 * Add test memories to Mnemo
 *
 * This script adds some sample memories to test that memory injection
 * is working correctly during chat completions.
 *
 * Usage:
 *   deno run --allow-run add-memory.ts
 *
 * Note: Requires mnemo-cli to be built (cargo build --release)
 */

const CLI_PATH = "../../target/release/mnemo-cli";
const DATA_DIR = "./test-data";

interface Memory {
  content: string;
  type: "semantic" | "episodic" | "procedural";
}

const testMemories: Memory[] = [
  {
    content: "The user's name is Alex and they prefer dark mode in all applications.",
    type: "semantic",
  },
  {
    content: "User mentioned they are building a Rust HTTP proxy called mnemo for LLM context injection.",
    type: "episodic",
  },
  {
    content: "User's favorite programming languages are Rust and TypeScript, in that order.",
    type: "semantic",
  },
  {
    content: "To run mnemo tests: cargo test --workspace -- --test-threads=1",
    type: "procedural",
  },
  {
    content: "User prefers concise, technical explanations without excessive examples.",
    type: "semantic",
  },
];

async function runCommand(args: string[]): Promise<{ success: boolean; output: string }> {
  try {
    const command = new Deno.Command(CLI_PATH, {
      args: ["--data-dir", DATA_DIR, ...args],
      stdout: "piped",
      stderr: "piped",
    });

    const { code, stdout, stderr } = await command.output();
    const output = new TextDecoder().decode(code === 0 ? stdout : stderr);

    return { success: code === 0, output: output.trim() };
  } catch (error) {
    return {
      success: false,
      output: error instanceof Error ? error.message : String(error),
    };
  }
}

async function main() {
  console.log("=".repeat(60));
  console.log("Adding Test Memories to Mnemo");
  console.log("=".repeat(60));
  console.log();

  // Check if CLI exists
  try {
    await Deno.stat(CLI_PATH);
  } catch {
    console.error(`Error: mnemo-cli not found at ${CLI_PATH}`);
    console.error("Build it first: cargo build --release");
    Deno.exit(1);
  }

  let successCount = 0;

  for (const memory of testMemories) {
    console.log(`Adding ${memory.type} memory...`);
    console.log(`  "${memory.content.substring(0, 50)}..."`);

    const result = await runCommand([
      "memory",
      "add",
      memory.content,
      "--type",
      memory.type,
    ]);

    if (result.success) {
      console.log("  Status: OK");
      successCount++;
    } else {
      console.log(`  Status: FAILED - ${result.output}`);
    }
    console.log();
  }

  console.log("=".repeat(60));
  console.log(`Added ${successCount}/${testMemories.length} memories`);
  console.log();

  // List memories to verify
  console.log("Verifying memories...");
  const listResult = await runCommand(["memory", "list"]);
  if (listResult.success) {
    console.log(listResult.output);
  } else {
    console.log("Failed to list memories:", listResult.output);
  }
}

main();
