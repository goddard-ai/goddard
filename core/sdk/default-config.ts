import { Models, defineConfig } from "@goddard-ai/config"

export default defineConfig({
  agent: {
    model: Models.OpenAi.Gpt53Codex,
    projectDir: "./",
    thinkingLevel: "low",
  },
  nextPrompt: () => "Make one safe improvement.",
  rateLimits: {
    cycleDelay: "30m",
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100,
  },
  retries: {
    maxAttempts: 3,
    initialDelayMs: 1000,
    maxDelayMs: 30000,
    backoffFactor: 2,
    jitterRatio: 0.2,
    retryableErrors: () => true,
  },
  metrics: {
    enableLogging: true,
  },
})
