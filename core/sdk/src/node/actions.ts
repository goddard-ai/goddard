import type { InlineSessionParams } from "@goddard-ai/schema/config"
import { runDaemonAction } from "../daemon/actions.ts"
import { type NodeDaemonClientOptions, resolveNodeDaemonClient } from "./client.ts"

/** Runs one named action through the daemon using Node cwd and env defaults when omitted. */
export async function runAgentAction(
  actionName: string,
  params: InlineSessionParams = {},
  options?: NodeDaemonClientOptions,
): Promise<null> {
  return runDaemonAction(
    actionName,
    {
      ...params,
      cwd: params.cwd ?? process.cwd(),
    },
    { client: resolveNodeDaemonClient(options) },
  )
}
