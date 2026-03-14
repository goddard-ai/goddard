const fs = require('fs');

let sdkLoop = fs.readFileSync('core/sdk/src/node/loop.ts', 'utf8');

sdkLoop = sdkLoop.replace(
  'import { createLoop, type GoddardLoopConfig } from "@goddard-ai/loop"',
  'import { createLoop, type GoddardLoopConfig } from "@goddard-ai/loop"\nimport { LoopStorage } from "@goddard-ai/storage"'
);

// We need to persist the loaded loop config into LoopStorage when loadLoopConfig is called
sdkLoop = sdkLoop.replace(
  'return { config: config as GoddardLoopConfig, path: configPath }',
  `
  const typedConfig = config as GoddardLoopConfig;

  // Persist to loop storage
  const existing = await LoopStorage.get(typedConfig.id);
  if (existing) {
    await LoopStorage.update(typedConfig.id, {
      agent: typedConfig.agent,
      systemPrompt: typedConfig.systemPrompt,
      strategy: typedConfig.strategy,
      displayName: typedConfig.displayName,
      cwd: typedConfig.cwd,
      mcpServers: typedConfig.mcpServers,
      gitRemote: typedConfig.gitRemote
    });
  } else {
    await LoopStorage.create({
      id: typedConfig.id,
      agent: typedConfig.agent,
      systemPrompt: typedConfig.systemPrompt,
      strategy: typedConfig.strategy ?? "",
      displayName: typedConfig.displayName,
      cwd: typedConfig.cwd,
      mcpServers: typedConfig.mcpServers,
      gitRemote: typedConfig.gitRemote
    });
  }

  return { config: typedConfig, path: configPath }
  `
);

fs.writeFileSync('core/sdk/src/node/loop.ts', sdkLoop);
