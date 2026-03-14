const fs = require('fs');

let sdkLoop = fs.readFileSync('core/sdk/src/node/loop.ts', 'utf8');

// We are changing loadLoopConfig to load from DB or create a default, not read config.ts
sdkLoop = sdkLoop.replace(
  'import { createLoop, type GoddardLoopConfig } from "@goddard-ai/loop"',
  'import { createLoop, type LoopRuntimeConfig } from "@goddard-ai/loop"'
);

// We need to fetch loop config from LoopStorage, configSchema is empty now, `.goddard/config.ts` will not be parsed for configuration values.
const loadLoopConfigImpl = `export async function loadLoopConfig(
  cwd: string = process.cwd(),
  loopId: string = "default"
): Promise<{ config: LoopRuntimeConfig }> {
  let record = await LoopStorage.get(loopId);

  if (!record) {
    // Return a default runtime config
    return {
      config: {
        agent: "anthropic/claude-3-7-sonnet-20250219",
        cwd,
        systemPrompt: "Make one safe improvement. Reply SUMMARY|DONE when finished.",
        strategy: "",
        mcpServers: []
      }
    };
  }

  return {
    config: {
      agent: record.agent,
      cwd: record.cwd,
      systemPrompt: record.systemPrompt,
      strategy: record.strategy ?? undefined,
      mcpServers: record.mcpServers ?? []
    }
  }
}
`;

sdkLoop = sdkLoop.replace(/export async function loadLoopConfig\([\s\S]*?return \{ config: typedConfig, path: configPath \}\n  \n\}/, loadLoopConfigImpl);

// We need to remove the GoddardLoopConfig reference
sdkLoop = sdkLoop.replace('import DEFAULT_LOOP_CONFIG_TEMPLATE from "../../default-config.ts?raw"\n\nexport async function initLoopConfig(options: { global?: boolean }): Promise<{ path: string }> {\n  const targetPath = options.global ? getGlobalConfigPath() : getLocalConfigPath()\n\n  if (await fileExists(targetPath)) {\n    throw new Error(`Config file already exists at ${targetPath}`)\n  }\n\n  await mkdir(dirname(targetPath), { recursive: true })\n  await writeFile(targetPath, DEFAULT_LOOP_CONFIG_TEMPLATE, "utf-8")\n\n  return { path: targetPath }\n}\n', '');

// Update runLoop signature to accept loopId, remove cwd optional? Keep cwd.
sdkLoop = sdkLoop.replace(
  `export async function runLoop(
  cwd: string = process.cwd(),
  deps?: { createLoopRuntime?: typeof createLoop },
): Promise<void> {
  const { config } = await loadLoopConfig(cwd)
  const runtime = deps?.createLoopRuntime ?? createLoop
  const loop = runtime(config)
  await loop.start()
}`,
  `export async function runLoop(
  cwd: string = process.cwd(),
  loopId: string = "default",
  deps?: { createLoopRuntime?: typeof createLoop },
): Promise<void> {
  const { config } = await loadLoopConfig(cwd, loopId)
  const runtime = deps?.createLoopRuntime ?? createLoop
  const loop = runtime(config)
  await loop.start()
}`
);

sdkLoop = sdkLoop.replace(/export async function generateLoopSystemdService\([\s\S]*?\n\}/, '');

fs.writeFileSync('core/sdk/src/node/loop.ts', sdkLoop);
