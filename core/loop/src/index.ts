import type { GoddardLoopConfig } from "./types.ts";
import { configSchema } from "./types.ts";
import { RateLimiter } from "./rate-limiter.ts";
import { runAgent, type AgentSession } from "@goddard-ai/session";
import type * as acp from "@agentclientprotocol/sdk";
import type { SessionParams } from "@goddard-ai/schema/session-server";
import type { CycleStrategy } from "@goddard-ai/config";
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

export async function runAgentLoop(
  { session: sessionParams, strategy }: { session: SessionParams & { oneShot?: undefined }, strategy: CycleStrategy },
  handler?: acp.Client
): Promise<AgentSession> {
  const retryConfig = {
    maxAttempts: 1,
    initialDelayMs: 1000,
    maxDelayMs: 30_000,
    backoffFactor: 2,
    jitterRatio: 0.2
  };

  const status = {
    cycle: 0,
    tokensUsed: 0,
    uptime: 0,
    startTime: Date.now()
  };

  const session = await runAgent(sessionParams, handler);

  const endlessLoop = async (): Promise<void> => {
    let lastSummary: string | undefined;

    status.cycle = 0;
    status.tokensUsed = 0;
    status.uptime = 0;
    status.startTime = Date.now();

    const onSigint = () => {
      session.stop();
      process.exit(0);
    };
    process.on("SIGINT", onSigint);

    try {
      while (true) {
        status.cycle += 1;
        status.uptime = Date.now() - status.startTime;

        const promptMessage = strategy.nextPrompt({
          cycleNumber: status.cycle,
          lastSummary
        });

        let attempt = 0;
        while (true) {
          try {
            await session.prompt(promptMessage);
            break;
          } catch (error) {
            if (
              error instanceof Error &&
              (error.name === "AbortError" || error.message.toLowerCase().includes("abort"))
            ) {
              return;
            }

            attempt += 1;

            if (attempt >= retryConfig.maxAttempts) {
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

        const history = await session.getHistory();
        const lastMessage = history[history.length - 1];
        let assistantText = "";

        if (lastMessage && "role" in lastMessage && lastMessage.role === "assistant" && "content" in lastMessage && Array.isArray(lastMessage.content)) {
          const textBlock = (lastMessage as any).content.find((b: any) => b.type === "text");
          if (textBlock && 'text' in textBlock) {
             assistantText = textBlock.text;
          }
        }

        lastSummary = assistantText || `Completed cycle ${status.cycle}`;

        if (isDoneSignal(lastSummary)) {
          return;
        }
      }
    } finally {
      process.removeListener("SIGINT", onSigint);
    }
  };

  // Start the background loop immediately
  endlessLoop().catch(console.error);

  return session;
}

export function createGoddardConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config;
}

export type { CycleContext, CycleStrategy, GoddardLoopConfig, PiAgentConfig } from "./types.ts";
export { DefaultStrategy } from "./strategies.ts";
export { Models, type Model } from "@goddard-ai/config";
export { LOOP_SYSTEM_PROMPT } from "./prompts.ts";