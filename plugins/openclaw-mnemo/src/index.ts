/**
 * Mnemo Memory Proxy Plugin for OpenClaw
 *
 * Integrates Mnemo (semantic memory injection) with OpenClaw's gateway.
 * Routes LLM requests through Mnemo for automatic memory injection.
 */

// Plugin configuration interface
interface MnemoConfig {
  url: string;
  enabled: boolean;
  healthCheckInterval: number;
  providers: {
    openai: boolean;
    anthropic: boolean;
  };
}

// Health check response from Mnemo
interface HealthStatus {
  available: boolean;
  url: string;
  lastCheck: Date;
  error?: string;
}

// Plugin API types (minimal for compilation without openclaw dep)
interface PluginApi {
  config: Record<string, unknown>;
  logger: {
    info: (msg: string, ...args: unknown[]) => void;
    warn: (msg: string, ...args: unknown[]) => void;
    error: (msg: string, ...args: unknown[]) => void;
    debug: (msg: string, ...args: unknown[]) => void;
  };
  registerCli: (
    handler: (ctx: { program: Commander }) => void,
    opts: { commands: string[] }
  ) => void;
  registerService: (service: {
    id: string;
    start: () => void | Promise<void>;
    stop: () => void | Promise<void>;
  }) => void;
  registerGatewayMethod: (
    method: string,
    handler: (ctx: { respond: (ok: boolean, data: unknown) => void }) => void
  ) => void;
}

interface Commander {
  command: (name: string) => CommanderCommand;
}

interface CommanderCommand {
  description: (desc: string) => CommanderCommand;
  option: (flags: string, desc: string, defaultValue?: unknown) => CommanderCommand;
  action: (fn: (...args: unknown[]) => void | Promise<void>) => CommanderCommand;
  command: (name: string) => CommanderCommand;
}

// Shared state for health monitoring
let healthStatus: HealthStatus = {
  available: false,
  url: "http://localhost:9999",
  lastCheck: new Date(),
};

let healthCheckTimer: ReturnType<typeof setInterval> | null = null;

/**
 * Check if Mnemo is available
 */
async function checkMnemoHealth(url: string): Promise<HealthStatus> {
  const healthUrl = `${url.replace(/\/$/, "")}/health`;

  try {
    const response = await fetch(healthUrl, {
      method: "GET",
      signal: AbortSignal.timeout(5000),
    });

    healthStatus = {
      available: response.ok,
      url,
      lastCheck: new Date(),
      error: response.ok ? undefined : `HTTP ${response.status}`,
    };
  } catch (err) {
    healthStatus = {
      available: false,
      url,
      lastCheck: new Date(),
      error: err instanceof Error ? err.message : "Unknown error",
    };
  }

  return healthStatus;
}

/**
 * Get the plugin config with defaults
 */
function getConfig(api: PluginApi): MnemoConfig {
  const pluginConfig = (api.config as Record<string, unknown>).plugins as
    | Record<string, unknown>
    | undefined;
  const entries = pluginConfig?.entries as Record<string, unknown> | undefined;
  const mnemoEntry = entries?.mnemo as Record<string, unknown> | undefined;
  const config = mnemoEntry?.config as Partial<MnemoConfig> | undefined;

  return {
    url: config?.url ?? "http://localhost:9999",
    enabled: config?.enabled ?? true,
    healthCheckInterval: config?.healthCheckInterval ?? 30000,
    providers: {
      openai: config?.providers?.openai ?? true,
      anthropic: config?.providers?.anthropic ?? true,
    },
  };
}

/**
 * Generate provider URL configuration
 */
function generateProviderUrls(mnemoUrl: string): {
  openai: string;
  anthropic: string;
} {
  const baseUrl = mnemoUrl.replace(/\/$/, "");
  return {
    openai: `${baseUrl}/p/https://api.openai.com/v1`,
    anthropic: `${baseUrl}/p/https://api.anthropic.com/v1`,
  };
}

/**
 * Main plugin registration
 */
export default function register(api: PluginApi): void {
  const config = getConfig(api);

  // Register CLI commands
  api.registerCli(
    ({ program }) => {
      const mnemoCmd = program
        .command("mnemo")
        .description("Mnemo memory proxy management");

      // Status command
      mnemoCmd
        .command("status")
        .description("Check Mnemo proxy health and configuration")
        .action(async () => {
          const cfg = getConfig(api);
          console.log("\n🧠 Mnemo Memory Proxy Status\n");
          console.log(`URL:     ${cfg.url}`);
          console.log(`Enabled: ${cfg.enabled ? "✓ Yes" : "✗ No"}`);
          console.log("");

          // Check health
          console.log("Checking connection...");
          const status = await checkMnemoHealth(cfg.url);

          if (status.available) {
            console.log(`Status:  ✓ Available`);
          } else {
            console.log(`Status:  ✗ Unavailable`);
            if (status.error) {
              console.log(`Error:   ${status.error}`);
            }
          }

          console.log("");
          console.log("Provider Routing:");
          console.log(`  OpenAI:    ${cfg.providers.openai ? "✓ Enabled" : "✗ Disabled"}`);
          console.log(`  Anthropic: ${cfg.providers.anthropic ? "✓ Enabled" : "✗ Disabled"}`);

          if (status.available) {
            const urls = generateProviderUrls(cfg.url);
            console.log("");
            console.log("Configured URLs:");
            console.log(`  OpenAI:    ${urls.openai}`);
            console.log(`  Anthropic: ${urls.anthropic}`);
          }

          console.log("");
        });

      mnemoCmd
        .command("configure")
        .description("Show configuration instructions for routing through Mnemo")
        .option("--url <url>", "Mnemo proxy URL", "http://localhost:9999")
        .action(async (...args: unknown[]) => {
          const options = (args[0] as { url?: string }) ?? {};
          const mnemoUrl = options.url ?? config.url;
          const urls = generateProviderUrls(mnemoUrl);

          console.log("\n🧠 Mnemo Configuration Guide\n");
          console.log("Add the following to your OpenClaw config:\n");
          console.log("```yaml");
          console.log("gateway:");
          console.log("  providers:");
          console.log("    openai:");
          console.log(`      baseUrl: "${urls.openai}"`);
          console.log("    anthropic:");
          console.log(`      baseUrl: "${urls.anthropic}"`);
          console.log("```\n");

          console.log("Plugin configuration (plugins.entries.mnemo.config):\n");
          console.log("```yaml");
          console.log("plugins:");
          console.log("  entries:");
          console.log("    mnemo:");
          console.log("      enabled: true");
          console.log("      config:");
          console.log(`        url: "${mnemoUrl}"`);
          console.log("        enabled: true");
          console.log("        healthCheckInterval: 30000");
          console.log("        providers:");
          console.log("          openai: true");
          console.log("          anthropic: true");
          console.log("```\n");

          // Check if Mnemo is running
          const status = await checkMnemoHealth(mnemoUrl);
          if (!status.available) {
            console.log("⚠️  Warning: Mnemo is not currently available at", mnemoUrl);
            console.log("   Make sure Mnemo is running before enabling the plugin.\n");
            console.log("   Start Mnemo with: cargo run --bin mnemo\n");
          } else {
            console.log("✓ Mnemo is running and available at", mnemoUrl);
            console.log("");
          }
        });
    },
    { commands: ["mnemo"] }
  );

  // Register background health monitoring service
  api.registerService({
    id: "mnemo-monitor",

    start: async () => {
      const cfg = getConfig(api);

      if (!cfg.enabled) {
        api.logger.info("Mnemo plugin disabled, skipping health monitor");
        return;
      }

      api.logger.info(`Starting Mnemo health monitor (interval: ${cfg.healthCheckInterval}ms)`);

      // Initial health check
      const status = await checkMnemoHealth(cfg.url);
      if (status.available) {
        api.logger.info("Mnemo proxy available at", cfg.url);
      } else {
        api.logger.warn("Mnemo proxy not available:", status.error);
      }

      // Start periodic health checks
      healthCheckTimer = setInterval(async () => {
        const prevAvailable = healthStatus.available;
        const status = await checkMnemoHealth(cfg.url);

        // Log state changes
        if (prevAvailable && !status.available) {
          api.logger.warn("Mnemo proxy became unavailable:", status.error);
        } else if (!prevAvailable && status.available) {
          api.logger.info("Mnemo proxy is now available");
        }
      }, cfg.healthCheckInterval);
    },

    stop: async () => {
      if (healthCheckTimer) {
        clearInterval(healthCheckTimer);
        healthCheckTimer = null;
      }
      api.logger.info("Mnemo health monitor stopped");
    },
  });

  // Register RPC method for status queries
  api.registerGatewayMethod("mnemo.status", ({ respond }) => {
    const cfg = getConfig(api);
    const urls = generateProviderUrls(cfg.url);

    respond(true, {
      config: {
        url: cfg.url,
        enabled: cfg.enabled,
        providers: cfg.providers,
      },
      health: {
        available: healthStatus.available,
        lastCheck: healthStatus.lastCheck.toISOString(),
        error: healthStatus.error,
      },
      providerUrls: urls,
    });
  });

  // Register RPC method for health check
  api.registerGatewayMethod("mnemo.health", async ({ respond }) => {
    const cfg = getConfig(api);
    const status = await checkMnemoHealth(cfg.url);

    respond(status.available, {
      available: status.available,
      url: status.url,
      lastCheck: status.lastCheck.toISOString(),
      error: status.error,
    });
  });

  api.logger.info("Mnemo plugin registered");
}

// Export plugin metadata
export const id = "mnemo";
export const name = "Mnemo Memory Proxy";
