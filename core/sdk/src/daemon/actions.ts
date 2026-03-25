import type { InlineSessionParams } from "@goddard-ai/schema/config"
import { resolveDaemonClient } from "./client.ts"
import type { DaemonClientOptions } from "./client.ts"

/** Runs one named action through the daemon without resolving local config in the SDK. */
export async function runAgentAction(
  actionName: string,
  params: InlineSessionParams = {},
  options?: DaemonClientOptions,
): Promise<null> {
  const client = resolveDaemonClient(options)
  await client.send("actionRun", {
    actionName,
    cwd: params.cwd ?? process.cwd(),
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
