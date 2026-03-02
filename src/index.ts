import { LoopConfig, TypedLoop, configSchema } from './types';
import { RateLimiter } from './rate-limiter';
import { createAgentSession } from '@mariozechner/pi-coding-agent';

export * from './types';

export function createLoop<Config extends LoopConfig>(
  config: Config
): TypedLoop<Config> {
  const validated = configSchema.parse(config);
  const limiter = new RateLimiter(validated.rateLimits);
  const strategy = validated.strategy;

  const status = {
    cycle: 0,
    tokensUsed: 0,
    uptime: 0,
    startTime: Date.now()
  };

  const endlessLoop = async ({ limiter, strategy }: any): Promise<never> => {
    let lastSummary: string | undefined = undefined;

    // Create session outside the loop so it persists context
    const { session } = await createAgentSession({
      cwd: validated.agent.projectDir,
      // model mapping would go here if needed
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
        // This is a placeholder for real pause logic
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

      await session.sendUserMessage(prompt);

      const stats = session.getSessionStats();
      status.tokensUsed = stats.tokens.total;
      lastSummary = session.getLastAssistantText() || `Completed cycle ${status.cycle}`;
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
