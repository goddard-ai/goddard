# pi-loop Quick Start

`pi-loop` is an endless, rate-limited agentic loop for `pi-coding-agent`. Follow these steps to get your first autonomous coding daemon running in under a minute!

## 1. Install Dependencies

You need `typescript`, `ts-node` (or `tsx`), `@types/node`, and `pi-loop` installed in your project:

```bash
pnpm install pi-loop typescript @types/node -D
```

*(Note: `pi-loop` assumes `@mariozechner/pi-coding-agent` is available in your environment).*

## 2. Initialize the Loop Configuration

Run the CLI to scaffold your fully-typed configuration:

```bash
npx pi-loop init
```

This will create:
- `pi-loop.config.ts`: Your main configuration file.
- `tsconfig.json`: TypeScript settings (if one didn't exist).
- `systemd/`: A directory for deployment configurations.

## 3. Review Your Configuration

Open the generated `pi-loop.config.ts` and adjust it to fit your needs. The default configuration looks like this:

```typescript
import { createLoop, createLoopConfig } from 'pi-loop';
import { DefaultStrategy } from 'pi-loop/strategies';

const config = createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    maxTokensPerCycle: 8000,
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m', // Throttles the loop (e.g. 30 minutes between cycles)
    maxTokensPerCycle: 8000,
    maxOpsPerMinute: 20,
    maxCyclesBeforePause: 24, // Pauses after 24 cycles
  },
  metrics: {
    prometheusPort: 9090,
    enableLogging: true,
  },
  systemd: {
    restartSec: 10,
    nice: 10,
  }
});

const loop = createLoop(config);
loop.start().catch(console.error);

export default config;
```

## 4. Run the Daemon

Execute the configuration file directly using `tsx` (or compile and run via node):

```bash
npx tsx pi-loop.config.ts
```

Your `pi-coding-agent` is now running autonomously in a supervised loop!
