import { createLoopConfig } from './src/index';
import { DefaultStrategy } from './src/strategies/index';

export default createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    maxTokensPerCycle: 8000,
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m', // Parsed by date-fns/cron
    maxTokensPerCycle: 8000,
    maxOpsPerMinute: 20,
    maxCyclesBeforePause: 24, // Daily pause
  },
  metrics: {
    prometheusPort: 9090,
    enableLogging: true,
  },
  systemd: {
    restartSec: 10,
    nice: 10, // Low CPU priority
  }
});
