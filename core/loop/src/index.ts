import type { GoddardLoop, GoddardLoopConfig } from "./types.ts";
import { configSchema } from "./types.ts";
import { RateLimiter } from "./rate-limiter.ts";
import { AuthStorage, ModelRegistry, createAgentSession, InteractiveMode } from "@mariozechner/pi-coding-agent";
import { join } from "node:path";
import { existsSync } from "node:fs";
import { homedir } from "node:os";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function withJitter(delayMs: number, jitterRatio: number): number {
  if (jitterRatio <= 0) {
    return delayMs;
  }

  const min = Math.max(0, delayMs * (1 - jitterRatio));
  const max = delayMs * (1 + jitterRatio);
  return Math.round(min + Math.random() * (max - min));
}

function resolveAgentDir(configuredDir?: string): string | undefined {
  if (configuredDir) {
    if (configuredDir.startsWith("~/")) {
      return join(homedir(), configuredDir.slice(2));
    }
    return configuredDir;
  }

  const piAgentDir = join(homedir(), ".pi", "agent");
  if (existsSync(piAgentDir)) {
    return piAgentDir;
  }

  return undefined;
}

function resolveConfiguredModel(modelRef: string, agentDir?: string) {
  const authPath = agentDir ? join(agentDir, "auth.json") : undefined;
  const modelsPath = agentDir ? join(agentDir, "models.json") : undefined;

  const authStorage = AuthStorage.create(authPath);
  const modelRegistry = new ModelRegistry(authStorage, modelsPath);

  if (modelRef.includes("/")) {
    const [provider, ...idParts] = modelRef.split("/");
    const modelId = idParts.join("/");

    if (!provider || !modelId) {
      throw new Error(`Invalid model format "${modelRef}". Use "provider/modelId" or "modelId".`);
    }

    const model = modelRegistry.find(provider, modelId);
    if (!model) {
      throw new Error(`Unknown model "${modelRef}". Verify provider/modelId in pi-coding-agent models.`);
    }

    return model;
  }

  const matches = modelRegistry.getAll().filter((model) => model.id === modelRef);

  if (matches.length === 0) {
    throw new Error(`Unknown model id "${modelRef}". Use "provider/modelId" for explicit selection.`);
  }

  if (matches.length > 1) {
    const options = matches.map((model) => `${model.provider}/${model.id}`).join(", ");
    throw new Error(`Ambiguous model id "${modelRef}". Use one of: ${options}`);
  }

  return matches[0];
}

function isDoneSignal(text: string | undefined): boolean {
  if (!text) {
    return false;
  }

  const normalized = text.trim();
  if (normalized.toUpperCase() === "DONE") {
    return true;
  }

  if (/^SUMMARY\s*\|\s*DONE$/i.test(normalized)) {
    return true;
  }

  return /(^|\n)\s*DONE\s*$/i.test(text);
}

export function createLoop(config: GoddardLoopConfig): GoddardLoop {
  const validated = configSchema.parse(config);
  const limiter = new RateLimiter(validated.rateLimits);
  const strategy = validated.strategy;
  const retryConfig = {
    maxAttempts: validated.retries?.maxAttempts ?? 1,
    initialDelayMs: validated.retries?.initialDelayMs ?? 1000,
    maxDelayMs: validated.retries?.maxDelayMs ?? 30_000,
    backoffFactor: validated.retries?.backoffFactor ?? 2,
    jitterRatio: validated.retries?.jitterRatio ?? 0.2,
    retryableErrors: validated.retries?.retryableErrors
  };

  const status = {
    cycle: 0,
    tokensUsed: 0,
    uptime: 0,
    startTime: Date.now()
  };

  const endlessLoop = async (): Promise<void> => {
    let lastSummary: string | undefined;

    status.cycle = 0;
    status.tokensUsed = 0;
    status.uptime = 0;
    status.startTime = Date.now();

    const resolvedAgentDir = resolveAgentDir(validated.agent.agentDir);
    const configuredModel = resolveConfiguredModel(validated.agent.model, resolvedAgentDir);

    const { session } = await createAgentSession({
      cwd: validated.agent.projectDir,
      model: configuredModel,
      thinkingLevel: validated.agent.thinkingLevel,
      agentDir: resolvedAgentDir
    });

    const ui = new InteractiveMode(session);
    await ui.init();

    const onSigint = () => {
      ui.stop();
      process.exit(0);
    };
    process.on("SIGINT", onSigint);

    try {
      while (true) {
        status.cycle += 1;
        status.uptime = Date.now() - status.startTime;

        const countdownPause = async (delayMs: number) => {
          ui.showWarning(`Rate limit reached. Pausing loop for ${Math.round(delayMs / 1000)} seconds...`);
          await sleep(delayMs);
        };

        if (status.cycle > 1) {
          await limiter.throttle(countdownPause);
        }

        if (
          validated.rateLimits.maxCyclesBeforePause &&
          status.cycle % validated.rateLimits.maxCyclesBeforePause === 0
        ) {
          await countdownPause(24 * 60 * 60 * 1000);
        }

        const prompt = strategy.nextPrompt({
          cycleNumber: status.cycle,
          lastSummary
        });

        const before = session.getSessionStats().tokens.total;

        let attempt = 0;
        while (true) {
          try {
            await session.sendUserMessage(prompt);
            break;
          } catch (error) {
            if (
              error instanceof Error &&
              (error.name === "AbortError" || error.message.toLowerCase().includes("abort"))
            ) {
              return;
            }

            attempt += 1;

            const isRetryable = retryConfig.retryableErrors
              ? retryConfig.retryableErrors(error, {
                  cycle: status.cycle,
                  attempt,
                  maxAttempts: retryConfig.maxAttempts
                })
              : true;

            if (!isRetryable || attempt >= retryConfig.maxAttempts) {
              throw error;
            }

            const baseDelay = Math.min(
              retryConfig.maxDelayMs,
              Math.round(retryConfig.initialDelayMs * Math.pow(retryConfig.backoffFactor, attempt - 1))
            );
            const retryDelay = withJitter(baseDelay, retryConfig.jitterRatio);

            await sleep(retryDelay);
          }
        }

        const stats = session.getSessionStats();
        const cycleTokens = stats.tokens.total - before;
        if (cycleTokens > validated.rateLimits.maxTokensPerCycle) {
          throw new Error(
            `[goddard loop] Cycle ${status.cycle} exceeded maxTokensPerCycle: used ${cycleTokens}, limit ${validated.rateLimits.maxTokensPerCycle}`
          );
        }

        status.tokensUsed = stats.tokens.total;
        lastSummary = session.getLastAssistantText() || `Completed cycle ${status.cycle}`;

        if (isDoneSignal(lastSummary)) {
          return;
        }
      }
    } finally {
      ui.stop();
      process.removeListener("SIGINT", onSigint);
    }
  };

  let isRunning = false;
  return {
    start: async () => {
      if (isRunning) {
        throw new Error("Loop is already running");
      }
      isRunning = true;
      try {
        await endlessLoop();
      } finally {
        isRunning = false;
      }
    },
    get status() {
      return {
        cycle: status.cycle,
        tokensUsed: status.tokensUsed,
        uptime: Date.now() - status.startTime
      };
    }
  };
}

export function createGoddardConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config;
}

export type { CycleContext, CycleStrategy, GoddardLoopConfig, PiAgentConfig } from "./types.ts";
export { DefaultStrategy } from "./strategies.ts";
export { Models, type Model } from "@goddard-ai/config";
export { LOOP_SYSTEM_PROMPT } from "./prompts.ts";
