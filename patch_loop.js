const fs = require('fs');

let index = fs.readFileSync('core/loop/src/index.ts', 'utf8');
index = index.replace(
  'const resolvedAgentDir = resolveAgentDir(validated.agent.agentDir);',
  'const resolvedAgentDir = resolveAgentDir(undefined);'
);

index = index.replace(
  'const configuredModel = resolveConfiguredModel(validated.agent.model, resolvedAgentDir);',
  'const configuredModel = resolveConfiguredModel(validated.agent, resolvedAgentDir);'
);

index = index.replace(
  'cwd: validated.agent.projectDir,',
  'cwd: validated.cwd,'
);

index = index.replace(
  'thinkingLevel: validated.agent.thinkingLevel,',
  'thinkingLevel: "medium" as const,'
);

index = index.replace(
  'const prompt = strategy.nextPrompt({',
  `// In absence of custom strategy logic, fallback to generic prompt
        const prompt = \`Cycle \${status.cycle}. Last: \${lastSummary ?? "none"}. \${validated.systemPrompt}\`; // Fallback`
);

index = index.replace(
  'export { Models, type Model } from "@goddard-ai/config";\n',
  ''
);

index = index.replace(
  'export type { CycleContext, CycleStrategy, GoddardLoopConfig, PiAgentConfig } from "./types.ts";',
  'export type { GoddardLoopConfig } from "./types.ts";\nexport type { CycleContext, CycleStrategy } from "./strategies.ts";'
);

fs.writeFileSync('core/loop/src/index.ts', index);


let types = fs.readFileSync('core/loop/src/types.ts', 'utf8');

types = `import type { GoddardLoopConfig } from "@goddard-ai/config";
import { configSchema } from "@goddard-ai/config";

export interface LoopStatus {
  cycle: number;
  tokensUsed: number;
  uptime: number;
}

export interface GoddardLoop {
  start: () => Promise<void>;
  status: LoopStatus;
}

export { configSchema };
export type { GoddardLoopConfig };
`;

fs.writeFileSync('core/loop/src/types.ts', types);
