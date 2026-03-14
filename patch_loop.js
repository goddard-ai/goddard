const fs = require('fs');

let index = fs.readFileSync('core/loop/src/index.ts', 'utf8');

index = index.replace(
  'import type { GoddardLoop, GoddardLoopConfig } from "./types.ts";\nimport { configSchema } from "./types.ts";',
  `import type { GoddardLoop } from "./types.ts";`
);

index = index.replace(
  'export function createLoop(config: GoddardLoopConfig): GoddardLoop {',
  `export interface LoopRuntimeConfig {
  agent: string;
  cwd: string;
  systemPrompt: string;
  strategy?: string;
  mcpServers?: any[];
}

export function createLoop(config: LoopRuntimeConfig): GoddardLoop {`
);

index = index.replace(
  `const validated = configSchema.parse(config);
  const limiter = new RateLimiter(validated.rateLimits);
  const strategy = validated.strategy;
  const retryConfig = {
    maxAttempts: validated.retries?.maxAttempts ?? 1,
    initialDelayMs: validated.retries?.initialDelayMs ?? 1000,
    maxDelayMs: validated.retries?.maxDelayMs ?? 30_000,
    backoffFactor: validated.retries?.backoffFactor ?? 2,
    jitterRatio: validated.retries?.jitterRatio ?? 0.2,
    retryableErrors: validated.retries?.retryableErrors
  };`,
  `const validated = config;
  const limiter = new RateLimiter({
    cycleDelay: "30m",
    maxTokensPerCycle: 128000,
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100,
  });
  const retryConfig = {
    maxAttempts: 3,
    initialDelayMs: 1000,
    maxDelayMs: 30000,
    backoffFactor: 2,
    jitterRatio: 0.2,
    retryableErrors: undefined
  };
  const rateLimits = {
    cycleDelay: "30m",
    maxTokensPerCycle: 128000,
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100,
  };
  `
);

index = index.replace(
  'validated.rateLimits',
  'rateLimits'
);
index = index.replace(
  'validated.rateLimits',
  'rateLimits'
);
index = index.replace(
  'validated.rateLimits',
  'rateLimits'
);
index = index.replace(
  'validated.rateLimits',
  'rateLimits'
);

index = index.replace(
  'export function createGoddardConfig(config: GoddardLoopConfig): GoddardLoopConfig {\n  return config;\n}',
  ''
);

index = index.replace(
  'export type { GoddardLoopConfig } from "./types.ts";\n',
  ''
);

fs.writeFileSync('core/loop/src/index.ts', index);


let types = fs.readFileSync('core/loop/src/types.ts', 'utf8');
types = types.replace(
  'import type { GoddardLoopConfig } from "@goddard-ai/config";\nimport { configSchema } from "@goddard-ai/config";\n',
  ''
);
types = types.replace(
  'export { configSchema };\nexport type { GoddardLoopConfig };',
  ''
);

fs.writeFileSync('core/loop/src/types.ts', types);
