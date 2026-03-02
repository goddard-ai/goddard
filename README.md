# pi-loop: Endless Rate-Limited Agentic Loop for pi-coding-agent

`pi-loop` is a zero-dependency npm package wrapping `@mariozechner/pi-coding-agent` in a configurable, endlessly running agentic loop with precise rate limiting.

**Core Value**: `pi-loop run` transforms a single-shot `agent.run()` into a production-grade daemon with zod-validated TypeScript configs.

## Features

- **Endless execution:** Safely loops `pi-coding-agent` with configurable delays between cycles.
- **Zero-Dependency TypeScript Configs:** Configuration is 100% typed out-of-the-box (`createLoopConfig()`).
- **Global or Local CLI:** Run `pi-loop` globally to target local (`./pi-loop.config.ts`) or global (`~/pi-loop.config.ts`) configurations.
- **Rate-Limiting Engine:** Prevents blowing past your token budgets or operations limits per minute.
- **Custom Strategies:** Control exactly what prompts are given to the agent each cycle.
- **Daemon Deployable:** Scaffolds configuration for easy deployment using `systemd`.

## Getting Started

If you want to quickly run a daemon, see our [QUICK_START.md](./QUICK_START.md).

```bash
npm i -g pi-loop
pi-loop init
pi-loop run
```

## Public TypeScript API

`pi-loop` exposes a tiny API surface area:

```typescript
import { createLoopConfig } from "pi-loop";
import { DefaultStrategy } from "pi-loop/strategies";

export default createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    maxTokensPerCycle: 8000,
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m',        // '30m', '2h', '1d' or cron
    maxTokensPerCycle: 8000,
    maxOpsPerMinute: 20,
    maxCyclesBeforePause: 24, // Pause after N cycles
  }
});
```

You can then run the configuration directly using the CLI:

```bash
pi-loop run
```

*Note: You can still invoke the programmatically via `createLoop(config).start()`!*

## CLI Configuration Scaffolding

```bash
pi-loop init
```

*Note: Use `pi-loop init --global` to save the config to `~/pi-loop.config.ts`.*

**Creates**:

```text
my-project/
└── pi-loop.config.ts         # Fully typed, zod-validated
```

## Strategy System

You can supply your own strategy by implementing the `CycleStrategy` interface:

```typescript
import type { CycleStrategy, CycleContext } from 'pi-loop';

export class MyStrategy implements CycleStrategy {
  nextPrompt(ctx: CycleContext): string {
    return `
      Cycle ${ctx.cycleNumber}.
      Last: ${ctx.lastSummary ?? 'none'}.
      codebase → ONE improvement → SUMMARY|DONE`;
  }
}
```

## Daemon Deployment (Config-Driven)

If you are using `systemd`, configure it inside `pi-loop.config.ts`:

```typescript
systemd: {
  restartSec: 10,
  nice: 10, // Low CPU priority
}
```

Then, you can generate the systemd daemon config file (which writes to `./systemd/pi-loop.service` or `~/systemd/pi-loop.service`):

```bash
pi-loop generate-systemd
sudo cp systemd/pi-loop.service /etc/systemd/system/
sudo systemctl enable pi-loop
```
