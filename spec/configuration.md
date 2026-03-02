# Configuration Contract

## Format

Configuration is authored in TypeScript (`pi-loop.config.ts`) and exported as default via `createLoopConfig(...)`.

## Discovery order

At runtime (`pi-loop run`):
1. `./pi-loop.config.ts` (current working directory)
2. `~/pi-loop.config.ts` (home directory)

Local config takes precedence over global config.

## Top-level shape

```ts
interface LoopConfig {
  agent: PiAgentConfig;
  strategy: CycleStrategy;
  rateLimits: {
    cycleDelay: string;
    maxTokensPerCycle: number;
    maxOpsPerMinute: number;
    maxCyclesBeforePause?: number;
  };
  metrics?: {
    prometheusPort?: number;
    enableLogging?: boolean;
  };
  systemd?: {
    restartSec?: number;
    nice?: number;
    user?: string;
    workingDir?: string;
    environment?: Record<string, string | undefined>;
  };
}
```

## Validation

`zod` validates required structure before loop startup.

- `agent` is passthrough (supports extra provider-specific fields).
- `strategy` must expose `nextPrompt`.
- `rateLimits` fields are required except `maxCyclesBeforePause`.
- If `agent.maxTokensPerCycle` is set, it must equal `rateLimits.maxTokensPerCycle`.

## Strategy contract

```ts
interface CycleStrategy {
  nextPrompt(ctx: { cycleNumber: number; lastSummary?: string }): string;
}
```

## Configuration guarantees

- Invalid configuration fails fast.
- TypeScript users get editor completion and static checks when using exported types/helpers.
