const fs = require('fs');

let sdkLoop = `import { createLoop, type LoopRuntimeConfig } from "@goddard-ai/loop"
import { LoopStorage } from "@goddard-ai/storage"


export async function loadLoopConfig(
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


export async function runLoop(
  cwd: string = process.cwd(),
  loopId: string = "default",
  deps?: { createLoopRuntime?: typeof createLoop },
): Promise<void> {
  const { config } = await loadLoopConfig(cwd, loopId)
  const runtime = deps?.createLoopRuntime ?? createLoop
  const loop = runtime(config)
  await loop.start()
}
`

fs.writeFileSync('core/sdk/src/node/loop.ts', sdkLoop);
