# pi-loop: Endless Rate-Limited Agentic Loop for pi-coding-agent

## Proposal Overview

`pi-loop` is a zero-dependency npm package wrapping Mario Zechner's `pi-coding-agent` in a configurable, endlessly running agentic loop with precise rate limiting. Exposes a 2-line TypeScript API for daemonizing autonomous coding agents. **100% TypeScript configuration** via `.ts` files with full type safety and IDE autocompletion.

**Core Value**: `await loop.start()` transforms single-shot `agent.run()` into production-grade daemon with zod-validated TypeScript configs.

## Technical Architecture

```
pi-loop/
├── src/
│   ├── index.ts           # createLoop<Config extends LoopConfig>()
│   ├── rate-limiter.ts
│   └── strategies/
├── types.ts              # LoopConfig, CycleStrategy interfaces
├── cli.ts               # `npx pi-loop init` generates config.ts
└── config.example.ts    # Full typed config template
```

## TypeScript-First Configuration

\*\*Generated `pi-ipt
import { createLoopConfig } from 'pi-loop/config';
import { DefaultStrategy } from 'pi-loop/strategies';

export default createLoopConfig({
agent: {
model: 'claude-sonnet-4',
projectDir: './',
maxTokensPerCycle: 8000,
},
strategy: new DefaultStrategy(),
rateLimits: {
cycleDelay: '30m', // Parsed by date-fns
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

````

**Full type definition**:
```typescript
export interface LoopConfig {
  agent: PiAgentConfig;
  strategy: CycleStrategy;
  rateLimits: {
    cycleDelay: string;        // '30m', '2h', '1d'
    maxTokensPerCycle: number;
    maxOpsPerMinute: number;
    maxCyclesBeforePause?: number;
  };
  metrics?: {
    prometheusPort?: number;
    enableLogging?: boolean;
  };
  systemd?: {
    restartSec: number;
    nice: number;
  };
}
````

## Public TypeScript API

```typescript
import { createLoop } from "pi-loop";
import config from "./pi-loop.config.ts"; // ✅ Full types + autocomplete

const loop = createLoop(config);
await loop.start(); // Inferred return type, never returns until SIGTERM

// type LoopStatus = { cycle: number, tokensUsed: number, uptime: number }
const status = loop.status;
```

## CLI Generates Typed Configs

```bash
npx pi-loop@latest init my-project
```

**Creates**:

```
my-project/
├── pi-loop.config.ts          # Fully typed, zod-validated
├── tsconfig.json             # Strict mode
├── package.json              # devDeps: typescript, @types/node
└── systemd/
    └── pi-loop.service      # Auto-generated from config.systemd
```

## Rate Limiting Engine (TypeScript Config-Driven)

```typescript
// In pi-loop.config.ts
rateLimits: {
  cycleDelay: '30m',                    // Wallclock throttling
  tokenBudget: { perCycle: 8000, burst: 12000 },
  opsBudget},
  resourceGuard: {
    maxCpuPercent: 70,
    maxRssMb: 500
  }
}
```

**Parsed at runtime**:

```typescript
// Internal: rateLimiter.fromConfig(config.rateLimits)
class MultiAxisLimiter {
  private tokenLimiter = pLimit({ concurrency: 1 });
  private opsLimiter = slidingWindow(20, "1m");
  private wallclock = cronParser(config.cycleDelay);
}
```

## Strategy System (TypeScript Classes)

```typescript
// strategies/my-strategy.ts - Full IDE support
import type { CycleStrategy, CycleContext } from 'pi-loop';

export class MyStrategy implements CycleStrategy {
  nextPrompt(ctx: CycleContext): string {
    return `
      Cycle ${ctx.cycleNumber}.
      Last: ${ctx.lastSummary ?? 'none'}.
      codebase → ONE improvement → SUMMARY|DONE`;
  }
}

// pi-loop.config.ts
strategy: new MyStrategy(),
```

## Package Structure

```
pi-loop/
├── src/
├── types.ts                 # All public interfaces
├── dist/
│   ├── index.d.ts          # Full declaration emit
│   └── index.jspi-loop.config.ts
├── templates/              # CLI scaffolding
└── tsconfig.json          # "strict": true, "noImplicitAny": true
```

**package.json**:

```json
{
  "types": "./dist/index.d.ts",
  "typings": "./dist/index.d.ts",
  "files": ["dist", "config.example.ts"],
  "peerDependencies": {
    "@mariozechner/pi-coding-agent": "^1.0.0",
    "typescript": ">=5.0.0"
  }
}
```

## Build & Type Distribution

```bash
# tsup config (zero-config TS→JS + types)
pnpm tsup src/index.ts --format esm,cjs \
  --dts --sourcemap \
  --target node20
```

## Daemon Deployment (Config-Driven)

```typescript
// pi-loop.config.ts
systemd: {
  user: 'nodeuser',
  workingDir: '/opt/my-project',
  environment: {
    ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY,
    CYCLE_DELAY: '1h'
  }
}
```

**CLI generates**:

```bash
npx pi-loop generate-systemd  # → pi-loop.service
sudo cp pi-loop.service /etc/systemd/system/
sudo systemctl enable pi-loop
```

## Implementation MVP (TypeScript-First)

```typescript
// srcLOC core
export function createLoop<Config extends LoopConfig>(
  config: Config,
): TypedLoop<Config> {
  const validated = configSchema.parse(config);
  const limiter = new RateLimiter(validated.rateLimits);
  const strategy = validated.strategy;

  return {
    start: async () => endlessLoop({ limiter, strategy }),
    status: createStatus(validated),
  };
}
```

This delivers **zero-JS-config, 100% TypeScript** daemonization with full IDE integration, zod validation, and pi-agent deep integration. Ready for `pnpm add pi-loop` + `cp config.example.ts config.ts` + edit + `await loop.start()`.
