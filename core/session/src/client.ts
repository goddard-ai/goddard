import * as acp from "@agentclientprotocol/sdk"
import type { SessionParams } from "@goddard-ai/schema/session-server"
import { PassThrough, Readable, Writable } from "node:stream"
import { AgentSession } from "./client-session.js"
import { createDaemonIpcClient } from "./daemon-ipc.js"

function shouldExitAfterInitialPrompt(params: SessionParams): boolean {
  return "sessionId" in params === false && params.oneShot === true
}

function createMessageInputTransport(client: ReturnType<typeof createDaemonIpcClient>["client"], id: string) {
  let buffer = ""

  return new Writable({
    write(chunk, _encoding, callback) {
      void (async () => {
        buffer += chunk.toString()
        const lines = buffer.split("\n")
        buffer = lines.pop() ?? ""

        for (const line of lines) {
          const trimmed = line.trim()
          if (!trimmed) {
            continue
          }

          await client.send("sessionSend", {
            id,
            message: JSON.parse(trimmed),
          })
        }
      })()
        .then(() => callback())
        .catch((error) => callback(error as Error))
    },
  })
}

export async function runAgent(
  params: SessionParams & { oneShot: true },
  handler?: acp.Client,
): Promise<null>

export async function runAgent(
  params: SessionParams & { oneShot?: undefined },
  handler?: acp.Client,
): Promise<AgentSession>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
): Promise<AgentSession | null>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
): Promise<AgentSession | null> {
  const { client } = createDaemonIpcClient()

  const connectedSession =
    "sessionId" in params && params.sessionId !== undefined
      ? await client.send("sessionConnect", { id: params.sessionId })
      : await client.send("sessionCreate", {
          agent: params.agent,
          cwd: params.cwd,
          mcpServers: params.mcpServers,
          systemPrompt: params.systemPrompt,
          env: params.env,
          metadata: params.metadata,
          initialPrompt: params.initialPrompt,
          oneShot: params.oneShot,
        })

  if (shouldExitAfterInitialPrompt(params)) {
    return null
  }

  const daemonSessionId = connectedSession.session.id
  const acpSessionId = connectedSession.session.acpId

  const agentInput = createMessageInputTransport(client, daemonSessionId)
  const agentOutput = new PassThrough()

  const unsubscribe = await client.subscribe("sessionMessage", ({ id, message }) => {
    if (id !== daemonSessionId) {
      return
    }

    agentOutput.write(`${JSON.stringify(message)}\n`)
  })

  const acpClient = new acp.ClientSideConnection(
    () =>
      handler ?? {
        async requestPermission() {
          return { outcome: { outcome: "cancelled" } }
        },
        async sessionUpdate() {
          // no-op by default
        },
      },
    acp.ndJsonStream(
      Writable.toWeb(agentInput),
      Readable.toWeb(agentOutput) as ReadableStream<Uint8Array>,
    ),
  )

  return new AgentSession(daemonSessionId, acpSessionId, acpClient, client, unsubscribe)
}
