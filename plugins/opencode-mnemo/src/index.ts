import type { Plugin } from "@opencode-ai/plugin";

const MNEMO_DEFAULT_HOST = "localhost";
const MNEMO_DEFAULT_PORT = 9999;

interface MnemoHealthResponse {
  status: string;
  version?: string;
}

/**
 * Check if Mnemo is running and reachable at the given host/port
 */
async function checkMnemoHealth(
  host: string = MNEMO_DEFAULT_HOST,
  port: number = MNEMO_DEFAULT_PORT
): Promise<{ running: boolean; version?: string; error?: string }> {
  try {
    const response = await fetch(`http://${host}:${port}/health`, {
      method: "GET",
      signal: AbortSignal.timeout(2000),
    });

    if (response.ok) {
      try {
        const data = (await response.json()) as MnemoHealthResponse;
        return { running: true, version: data.version };
      } catch {
        return { running: true };
      }
    }

    return { running: false, error: `HTTP ${response.status}` };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { running: false, error: message };
  }
}

/**
 * Generate the Mnemo proxy URL for a given provider base URL
 */
function getMnemoProxyUrl(
  providerBaseUrl: string,
  mnemoHost: string = MNEMO_DEFAULT_HOST,
  mnemoPort: number = MNEMO_DEFAULT_PORT
): string {
  return `http://${mnemoHost}:${mnemoPort}/p/${providerBaseUrl}`;
}

/**
 * Get pre-configured provider settings for Anthropic and OpenAI
 */
function getProviderConfigs(
  mnemoHost: string = MNEMO_DEFAULT_HOST,
  mnemoPort: number = MNEMO_DEFAULT_PORT
): Record<string, { baseURL: string }> {
  return {
    anthropic: {
      baseURL: getMnemoProxyUrl(
        "https://api.anthropic.com/v1",
        mnemoHost,
        mnemoPort
      ),
    },
    openai: {
      baseURL: getMnemoProxyUrl(
        "https://api.openai.com/v1",
        mnemoHost,
        mnemoPort
      ),
    },
  };
}

type LogLevel = "debug" | "info" | "warn" | "error";

interface LogClient {
  app: {
    log: (options: {
      body: {
        service: string;
        level: LogLevel;
        message: string;
        extra?: Record<string, unknown>;
      };
    }) => Promise<unknown>;
  };
}

function log(
  client: LogClient,
  level: LogLevel,
  message: string,
  extra?: Record<string, unknown>
): Promise<unknown> {
  return client.app.log({
    body: {
      service: "opencode-mnemo",
      level,
      message,
      extra,
    },
  });
}

/**
 * OpenCode plugin for Mnemo - LLM memory proxy integration.
 * Detects Mnemo availability and logs helpful configuration guidance.
 */
export const MnemoPlugin: Plugin = async ({ client }) => {
  const mnemoHost = process.env.MNEMO_HOST ?? MNEMO_DEFAULT_HOST;
  const mnemoPort = parseInt(
    process.env.MNEMO_PORT ?? String(MNEMO_DEFAULT_PORT),
    10
  );

  const initialHealth = await checkMnemoHealth(mnemoHost, mnemoPort);

  if (initialHealth.running) {
    await log(
      client as unknown as LogClient,
      "info",
      `Mnemo detected at ${mnemoHost}:${mnemoPort}`,
      { version: initialHealth.version }
    );
  } else {
    await log(
      client as unknown as LogClient,
      "debug",
      `Mnemo not detected at ${mnemoHost}:${mnemoPort}`,
      { error: initialHealth.error }
    );
  }

  return {
    event: async ({ event }) => {
      if (event.type === "session.created") {
        const health = await checkMnemoHealth(mnemoHost, mnemoPort);

        if (health.running) {
          const versionInfo = health.version ? ` (v${health.version})` : "";
          await log(
            client as unknown as LogClient,
            "info",
            `Mnemo memory proxy active${versionInfo}`
          );
        } else {
          await log(
            client as unknown as LogClient,
            "warn",
            "Mnemo is not running - memory features unavailable",
            {
              suggestion:
                "Start Mnemo with: cargo run --bin mnemo (or mnemo if installed)",
              configHint:
                "Then configure your provider baseURL to use Mnemo proxy",
            }
          );

          const configs = getProviderConfigs(mnemoHost, mnemoPort);
          await log(
            client as unknown as LogClient,
            "info",
            "To enable Mnemo, add to opencode.json:",
            {
              example: {
                provider: {
                  anthropic: { options: configs.anthropic },
                },
              },
            }
          );
        }
      }

      if (event.type === "server.connected") {
        const health = await checkMnemoHealth(mnemoHost, mnemoPort);

        await log(
          client as unknown as LogClient,
          "debug",
          `Mnemo integration status: ${health.running ? "active" : "inactive"}`,
          {
            host: mnemoHost,
            port: mnemoPort,
            ...(health.version && { version: health.version }),
            ...(health.error && { error: health.error }),
          }
        );
      }
    },
  };
};

export { checkMnemoHealth, getMnemoProxyUrl, getProviderConfigs };
export const MNEMO_DEFAULTS = {
  host: MNEMO_DEFAULT_HOST,
  port: MNEMO_DEFAULT_PORT,
};
