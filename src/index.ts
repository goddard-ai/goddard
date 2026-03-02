import { LoopConfig, TypedLoop, configSchema } from './types';
import { RateLimiter } from './rate-limiter';
import { AuthStorage, ModelRegistry, createAgentSession } from '@mariozechner/pi-coding-agent';

export * from './types';

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

function withJitter(delayMs: number, jitterRatio: number): number {
  if (jitterRatio <= 0) {
    return delayMs;
  }

  const min = Math.max(0, delayMs * (1 - jitterRatio));
  const max = delayMs * (1 + jitterRatio);
  return Math.round(min + Math.random() * (max - min));
}

function resolveConfiguredModel(modelRef: string) {
  const authStorage = AuthStorage.create();
  const modelRegistry = new ModelRegistry(authStorage);

  if (modelRef.includes('/')) {
    const [provider, ...idParts] = modelRef.split('/');
    const modelId = idParts.join('/');

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
    const options = matches.map((model) => `${model.provider}/${model.id}`).join(', ');
    throw new Error(`Ambiguous model id "${modelRef}". Use one of: ${options}`);
  }

  return matches[0];
}

function isDoneSignal(text: string | undefined): boolean {
  if (!text) {
    return false;
  }

  const normalized = text.trim();
  if (normalized.toUpperCase() === 'DONE') {
    return true;
  }

  if (/^SUMMARY\s*\|\s*DONE$/i.test(normalized)) {
    return true;
  }

  return /(^|\n)\s*DONE\s*$/i.test(text);
}

export function createLoop<Config extends LoopConfig>(
  config: Config
): TypedLoop<Config> {
  const validated = configSchema.parse(config);
  const limiter = new RateLimiter(validated.rateLimits);
  const strategy = validated.strategy;
  const retryConfig = {
    maxAttempts: validated.retries?.maxAttempts ?? 1,
    initialDelayMs: validated.retries?.initialDelayMs ?? 1000,
    maxDelayMs: validated.retries?.maxDelayMs ?? 30000,
    backoffFactor: validated.retries?.backoffFactor ?? 2,
    jitterRatio: validated.retries?.jitterRatio ?? 0.2,
  };

  const status = {
    cycle: 0,
    tokensUsed: 0,
    uptime: 0,
    startTime: Date.now()
  };

  const endlessLoop = async ({ limiter, strategy }: { limiter: RateLimiter; strategy: Config['strategy'] }): Promise<void> => {
    let lastSummary: string | undefined = undefined;

    const configuredModel = resolveConfiguredModel(validated.agent.model);

    // Create session outside the loop so it persists context
    const { session } = await createAgentSession({
      cwd: validated.agent.projectDir,
      model: configuredModel,
    });

    while (true) {
      status.cycle++;
      status.uptime = Date.now() - status.startTime;

      // Throttle
      await limiter.throttle();

      // Check max cycles pause
      if (
        validated.rateLimits.maxCyclesBeforePause &&
        status.cycle % validated.rateLimits.maxCyclesBeforePause === 0
      ) {
        // Simple pause logic (e.g. sleep 24h)
        await new Promise(r => setTimeout(r, 1000 * 60 * 60 * 24));
      }

      // Next prompt from strategy
      const prompt = strategy.nextPrompt({
        cycleNumber: status.cycle,
        lastSummary
      });

      // Run pi-coding-agent
      if (validated.metrics?.enableLogging) {
        console.log(`[pi-loop] Cycle ${status.cycle}: Starting...`);
        console.log(`[pi-loop] Prompt: ${prompt}`);
      }

      const before = session.getSessionStats().tokens.total;

      let attempt = 0;
      while (true) {
        try {
          await session.sendUserMessage(prompt);
          break;
        } catch (error) {
          attempt++;
          if (attempt >= retryConfig.maxAttempts) {
            throw error;
          }

          const baseDelay = Math.min(
            retryConfig.maxDelayMs,
            Math.round(retryConfig.initialDelayMs * Math.pow(retryConfig.backoffFactor, attempt - 1))
          );
          const retryDelay = withJitter(baseDelay, retryConfig.jitterRatio);

          if (validated.metrics?.enableLogging) {
            const message = error instanceof Error ? error.message : String(error);
            console.warn(
              `[pi-loop] Cycle ${status.cycle} failed (attempt ${attempt}/${retryConfig.maxAttempts}). Retrying in ${retryDelay}ms: ${message}`
            );
          }

          await sleep(retryDelay);
        }
      }

      const stats = session.getSessionStats();
      const cycleTokens = stats.tokens.total - before;

      if (cycleTokens > validated.rateLimits.maxTokensPerCycle) {
        throw new Error(
          `[pi-loop] Cycle ${status.cycle} exceeded maxTokensPerCycle: used ${cycleTokens}, limit ${validated.rateLimits.maxTokensPerCycle}`
        );
      }

      status.tokensUsed = stats.tokens.total;
      lastSummary = session.getLastAssistantText() || `Completed cycle ${status.cycle}`;

      if (isDoneSignal(lastSummary)) {
        if (validated.metrics?.enableLogging) {
          console.log('[pi-loop] Completed: received DONE signal from strategy response.');
          console.log(`[pi-loop] Final summary: ${lastSummary}`);
        }
        return;
      }
    }
  };

  return {
    start: async () => endlessLoop({ limiter, strategy }),
    get status() {
      return {
        ...status,
        uptime: Date.now() - status.startTime
      };
    },
  };
}

export function createLoopConfig<T extends LoopConfig>(config: T): T {
  return config;
}
