/** ACP adapter inspection helpers used by the repo-level `acp` development CLI. */
import * as os from "node:os"
import * as acp from "@agentclientprotocol/sdk"

import { createAgentMessageStream } from "./acp.ts"
import { spawnAgentProcess } from "./manager.ts"
import { createACPRegistryService } from "./registry.ts"

/** Starts one raw ACP adapter and returns a client connection for inspection commands. */
async function startAdapterInspection(adapter: string, cwd: string) {
  const processHandle = await spawnAgentProcess({
    daemonUrl: "http://localhost:0",
    token: "test-token",
    agent: adapter,
    cwd,
    agentBinDir: os.tmpdir(),
    registryService: createACPRegistryService(),
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

  return {
    connection,
    sessionUpdates,
    close() {
      processHandle.kill()
    },
  }
}

/** Starts one raw ACP adapter, initializes it, and opens a fresh session for inspection. */
export async function inspectAdapterSession(adapter: string, cwd: string) {
  const inspection = await startAdapterInspection(adapter, cwd)

  try {
    const initialize = await inspection.connection.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: {
        name: "goddard-acp",
        version: "1.0.0",
      },
    })
    const session = await inspection.connection.newSession({
      cwd,
      mcpServers: [],
    })

    return {
      initialize,
      session,
      sessionUpdates: inspection.sessionUpdates,
      close: inspection.close,
    }
  } catch (error) {
    inspection.close()
    throw error
  }
}

/** Calls ACP `session/list` on one raw adapter without creating a new session. */
export async function listAdapterSessions(
  adapter: string,
  cwd: string,
  request: acp.ListSessionsRequest,
) {
  const inspection = await startAdapterInspection(adapter, cwd)

  try {
    const initialize = await inspection.connection.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: {
        name: "goddard-acp",
        version: "1.0.0",
      },
    })

    if (initialize.agentCapabilities?.sessionCapabilities?.list == null) {
      throw new Error(`Adapter ${adapter} does not advertise ACP session/list support`)
    }

    const sessionList = await inspection.connection.listSessions(request)

    return {
      initialize,
      sessionList,
      sessionUpdates: inspection.sessionUpdates,
      close: inspection.close,
    }
  } catch (error) {
    inspection.close()
    throw error
  }
}
