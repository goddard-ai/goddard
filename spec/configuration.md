# Configuration Contract

## Format

Configuration is authored in TypeScript (`goddard.config.ts`) and exported as default via `createGoddardConfig(...)`.

---

## Discovery Order

At runtime (`goddard loop run`):
1. `./goddard.config.ts` — current working directory (local config).
2. `~/.goddard/config.ts` — home directory (global config).

Local config takes precedence over global config.

---

## Top-Level Shape

```ts
interface GoddardConfig {
  agent: PiAgentConfig;       // passed through to pi-coding-agent; supports extra provider fields
  strategy: CycleStrategy;    // must expose nextPrompt()

  rateLimits: {
    cycleDelay: string;           // duration shorthand (e.g. "30m") or cron expression
    maxTokensPerCycle: number;
    maxOpsPerMinute: number;
    maxCyclesBeforePause?: number;
  };

  retries?: {
    maxAttempts?: number;
    initialDelayMs?: number;
    maxDelayMs?: number;
    backoffFactor?: number;
    jitterRatio?: number;         // must be [0, 1]
    retryableErrors?: (
      error: unknown,
      context: { cycle: number; attempt: number; maxAttempts: number }
    ) => boolean;
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

---

## Validation Rules

All validation is performed by Zod before loop startup.

| Field | Rule |
|-------|------|
| `agent` | Passthrough — supports extra provider-specific fields without stripping. |
| `strategy` | Must expose a `nextPrompt` method. |
| `rateLimits.cycleDelay` | Required string; parsed as duration shorthand, then cron, then fallback to 60 s. |
| `rateLimits.maxTokensPerCycle` | Required positive number. |
| `rateLimits.maxOpsPerMinute` | Required positive number. |
| `agent.maxTokensPerCycle` | If set, must equal `rateLimits.maxTokensPerCycle`. |
| `retries.maxDelayMs` | Must be ≥ `initialDelayMs` when both are set. |
| `retries.jitterRatio` | Must be in range `[0, 1]`. |
| `retries.retryableErrors` | When provided, must be a function. |

---

## Strategy Contract

```ts
interface CycleStrategy {
  nextPrompt(ctx: { cycleNumber: number; lastSummary?: string }): string;
}
```

Strategies are pluggable: any object implementing `nextPrompt` can be passed as `config.strategy`. This allows custom classes, closures, or imported strategies without modifying loop internals.

---

## Configuration Guarantees

- **Fail fast:** Invalid config throws at startup, not mid-cycle.
- **IDE-friendly:** TypeScript users get editor completion and static type checks when using exported helpers.
- **Immutable after validation:** Config is treated as read-only once the loop has started.
