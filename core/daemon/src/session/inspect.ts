/** ACP adapter inspection helpers used by the repo-level `acp` development CLI. */
import * as acp from "@agentclientprotocol/sdk"
import * as os from "node:os"
import { createAgentMessageStream } from "./acp.ts"
import { spawnAgentProcess } from "./manager.ts"

/** Starts one raw ACP adapter, initializes it, and opens a fresh session for inspection. */
export async function inspectAdapterSession(adapter: string, cwd: string) {
  const processHandle = await spawnAgentProcess({
    daemonUrl: "http://localhost:0",
    token: "test-token",
    agent: adapter,
    cwd,
    agentBinDir: os.tmpdir(),
  })
  const stream = createAgentMessageStream(processHandle.stdin, processHandle.stdout)
  const sessionUpdates: acp.AnyMessage[] = []
  const connection = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params: unknown) {
        sessionUpdates.push({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params,
        } as acp.AnyMessage)
      },
    }),
    stream,
  )

  try {
    const initialize = await connection.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: {
        name: "goddard-acp",
        version: "1.0.0",
      },
    })
    const session = await connection.newSession({
      cwd,
      mcpServers: [],
    })

    return {
      initialize,
      session,
      sessionUpdates,
      close() {
        processHandle.kill()
      },
    }
  } catch (error) {
    processHandle.kill()
    throw error
  }
}
