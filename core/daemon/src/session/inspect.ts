/** ACP adapter inspection helpers used by the repo-level `acp` development CLI. */
import * as os from "node:os"
import * as acp from "@agentclientprotocol/sdk"
import { createAcpClient } from "acp-client"

import { spawnAgentProcess } from "./agent-process.ts"
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
  try {
    const sessionUpdates: acp.AnyMessage[] = []
    const client = await createAcpClient({
      stdin: processHandle.stdin,
      stdout: processHandle.stdout,
      clientInfo: {
        name: "goddard-acp",
        version: "1.0.0",
      },
      handler: {
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
      },
    })

    return {
      client,
      sessionUpdates,
      close() {
        void client.close()
        processHandle.kill()
      },
    }
  } catch (error) {
    processHandle.kill()
    throw error
  }
}

/** Starts one raw ACP adapter, initializes it, and opens a fresh session for inspection. */
export async function inspectAdapterSession(adapter: string, cwd: string) {
  const inspection = await startAdapterInspection(adapter, cwd)

  try {
    const session = await inspection.client.newSession({
      cwd,
      mcpServers: [],
    })

    return {
      initialize: inspection.client.initialize,
      session: { sessionId: session.sessionId },
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
    const initialize = inspection.client.initialize

    if (initialize.agentCapabilities?.sessionCapabilities?.list == null) {
      throw new Error(`Adapter ${adapter} does not advertise ACP session/list support`)
    }

    const sessionList = await inspection.client.listSessions(request)

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
