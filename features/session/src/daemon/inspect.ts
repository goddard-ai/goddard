/** ACP adapter inspection helpers used by the repo-level `acp` development CLI. */
import * as os from "node:os"
import { delimiter } from "node:path"
import type { ManagedAgentService } from "@goddard-ai/agent/daemon"
import { createAcpClient } from "acp-client"
import * as acp from "acp-client/protocol"

import { spawnAgentProcess } from "./agent-process.ts"

/** Starts one raw ACP adapter and returns a client connection for inspection commands. */
async function startAdapterInspection(
  adapter: string,
  cwd: string,
  managedAgent: ManagedAgentService,
) {
  const processHandle = await spawnAgentProcess({
    daemonUrl: "http://localhost:0",
    token: "test-token",
    agent: adapter,
    cwd,
    createAgentEnvironment: ({ env }) => ({
      ...env,
      PATH: [os.tmpdir(), env?.PATH ?? process.env.PATH].filter(Boolean).join(delimiter),
    }),
    managedAgent,
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
export async function inspectAdapterSession(
  adapter: string,
  cwd: string,
  managedAgent: ManagedAgentService,
) {
  const inspection = await startAdapterInspection(adapter, cwd, managedAgent)

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
  managedAgent: ManagedAgentService,
) {
  const inspection = await startAdapterInspection(adapter, cwd, managedAgent)

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
