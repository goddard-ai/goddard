# pi-loop Quick Start

`pi-loop` is an endless, rate-limited agentic loop for `pi-coding-agent`. Follow these steps to get your first autonomous coding daemon running in under a minute!

## 1. Install Dependencies

Install `pi-loop` globally so that you can use the CLI, and ensure `typescript` and `@mariozechner/pi-coding-agent` are available in your project.

```bash
npm install -g pi-loop
npm install typescript @types/node -D
```

## 2. Initialize the Loop Configuration

Run the CLI to scaffold your fully-typed configuration:

```bash
pi-loop init
```

This will create:
- `pi-loop.config.ts`: Your main configuration file.

*(You can also use `pi-loop init --global` to store the config in your home directory, e.g. `~/pi-loop.config.ts`)*

## 3. Review Your Configuration

Open the generated `pi-loop.config.ts` and adjust it to fit your needs. The default configuration looks like this:

```typescript
import { createLoopConfig } from 'pi-loop';
import { DefaultStrategy } from 'pi-loop/strategies';

export default createLoopConfig({
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
```

## 4. Run the Daemon

Start the loop using the CLI:

```bash
pi-loop run
```

This command automatically parses your `pi-loop.config.ts` (local takes precedence over global) and starts the loop! Your `pi-coding-agent` is now running autonomously in a supervised loop.
