import { Models, defineConfig } from "@goddard-ai/config"

export default defineConfig({
  agent: {
    model: Models.OpenAi.Gpt53Codex,
    projectDir: "./",
    thinkingLevel: "low",
  },
  loop: {
    strategy: {
      nextPrompt: ({ cycleNumber, lastSummary }) =>
        `Cycle ${cycleNumber}. Last summary: ${lastSummary ?? "none"}. Make one safe improvement, then answer with SUMMARY|DONE when ready.`,
    },
    rateLimits: {
      cycleDelay: "30m",
      maxTokensPerCycle: 128000,
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
  },
})
