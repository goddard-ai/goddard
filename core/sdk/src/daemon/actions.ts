import type { InlineSessionParams } from "@goddard-ai/schema/config"
import { resolveDaemonClient } from "./client.ts"
import type { DaemonClientOptions } from "./client.ts"

/** Runs one named action through the daemon without applying any local Node defaults. */
export async function runDaemonAction(
  actionName: string,
  params: InlineSessionParams,
  options: DaemonClientOptions,
): Promise<null> {
  const client = resolveDaemonClient(options)
  await client.send("actionRun", {
    actionName,
    cwd: params.cwd,
    agent: params.agent,
    mcpServers: params.mcpServers,
    env: params.env,
    systemPrompt: params.systemPrompt,
    repository: params.repository,
    prNumber: params.prNumber,
    metadata: params.metadata,
  })
  return null
}
