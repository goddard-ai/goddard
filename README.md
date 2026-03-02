# pi-loop: Endless Rate-Limited Agentic Loop for pi-coding-agent

`pi-loop` is a zero-dependency npm package wrapping `@mariozechner/pi-coding-agent` in a configurable, endlessly running agentic loop with precise rate limiting.

**Core Value**: `await loop.start()` transforms a single-shot `agent.run()` into a production-grade daemon with zod-validated TypeScript configs.

## Features

- **Endless execution:** Safely loops `pi-coding-agent` with configurable delays between cycles.
- **Zero-Dependency TypeScript Configs:** Configuration is 100% typed out-of-the-box (`createLoopConfig()`).
- **Rate-Limiting Engine:** Prevents blowing past your token budgets or operations limits per minute.
- **Custom Strategies:** Control exactly what prompts are given to the agent each cycle.
- **Daemon Deployable:** Scaffolds configuration for easy deployment using `systemd`.

## Getting Started

If you want to quickly run a daemon, see our [QUICK_START.md](./QUICK_START.md).

```bash
npx pi-loop init
```

## Public TypeScript API

`pi-loop` exposes a tiny API surface area:

```typescript
import { createLoop, createLoopConfig } from "pi-loop";
import { DefaultStrategy } from "pi-loop/strategies";

const config = createLoopConfig({
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

const loop = createLoop(config);

// type LoopStatus = { cycle: number, tokensUsed: number, uptime: number }
console.log(loop.status);

await loop.start(); // Inferred return type, never returns until SIGTERM
```

## CLI Configuration Scaffolding

```bash
npx pi-loop init my-project
```

**Creates**:

```text
my-project/
├── pi-loop.config.ts         # Fully typed, zod-validated
├── tsconfig.json             # Strict mode
└── systemd/
    └── pi-loop.service       # Auto-generated from config.systemd
```

You can then run the configuration directly using tools like `tsx`:

```bash
npx tsx pi-loop.config.ts
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

Then, you can generate the systemd daemon config file:

```bash
npx pi-loop generate-systemd
sudo cp systemd/pi-loop.service /etc/systemd/system/
sudo systemctl enable pi-loop
```
