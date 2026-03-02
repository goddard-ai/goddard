import { createLoopConfig } from './src/index';
import { DefaultStrategy } from './src/strategies/index';

export default createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    thinkingLevel: 'low',
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m', // Parsed by date-fns/cron
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
    retryableErrors: (error) => {
      return true;
    },
  },
});
