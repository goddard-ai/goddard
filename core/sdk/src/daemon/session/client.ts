import * as acp from "@agentclientprotocol/sdk"
import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import type { SessionParams } from "@goddard-ai/schema/session-server"
import { AgentSession } from "./client-session.ts"

/** Detects the session-creation case that returns no live client session object. */
function shouldExitAfterInitialPrompt(params: SessionParams): boolean {
  return "sessionId" in params === false && params.oneShot === true
}

/** Turns a writable ACP transport into daemon `sessionSend` requests. */
function createMessageInputTransport(
  client: DaemonIpcClient,
  id: DaemonSession["id"],
): WritableStream {
  let buffer = ""
  const decoder = new TextDecoder()

  return new WritableStream({
    async write(chunk) {
      buffer += decodeStreamChunk(chunk, decoder)
      buffer = await flushMessageBuffer(buffer, client, id)
    },
    async close() {
      const finalChunk = decoder.decode()
      if (finalChunk) {
        buffer += finalChunk
      }

      const trimmed = buffer.trim()
      if (!trimmed) {
        return
      }

      await client.send("sessionSend", {
        id,
        message: JSON.parse(trimmed),
      })
    },
  })
}

/** Flushes newline-delimited ACP messages from a partial input buffer. */
async function flushMessageBuffer(
  buffer: string,
  client: DaemonIpcClient,
  id: DaemonSession["id"],
): Promise<string> {
  const lines = buffer.split("\n")
  const remainingBuffer = lines.pop() ?? ""

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

  return remainingBuffer
}

/** Normalizes writable stream chunks into ACP NDJSON text. */
function decodeStreamChunk(chunk: unknown, decoder: TextDecoder): string {
  if (typeof chunk === "string") {
    return chunk
  }
  if (chunk instanceof Uint8Array) {
    return decoder.decode(chunk, { stream: true })
  }

  throw new Error(`Unsupported ACP transport chunk: ${String(chunk)}`)
}

/** Turns daemon-published session messages back into a readable ACP output transport. */
async function createMessageOutputTransport(
  client: DaemonIpcClient,
  id: DaemonSession["id"],
): Promise<{
  readable: ReadableStream<Uint8Array>
  close: () => Promise<void>
}> {
  const agentMethods = new Set<string>(Object.values(acp.AGENT_METHODS))
  const encoder = new TextEncoder()
  const stream = new TransformStream<Uint8Array, Uint8Array>()
  const writer = stream.writable.getWriter()
  let closed = false

  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id } },
    ({ message }) => {
      if (
        closed ||
        (typeof message === "object" &&
          message !== null &&
          "method" in message &&
          typeof message.method === "string" &&
          agentMethods.has(message.method))
      ) {
        return
      }

      void writer.write(encoder.encode(`${JSON.stringify(message)}\n`)).catch(() => {})
    },
  )

  return {
    readable: stream.readable,
    close: async () => {
      if (closed) {
        return
      }

      closed = true
      await Promise.resolve(unsubscribe()).catch(() => {})
      await writer.close().catch(() => {})
    },
  }
}

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams & { oneShot: true },
): Promise<null>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams & { oneShot: true },
  handler: acp.Client | undefined,
): Promise<null>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams & { oneShot?: undefined },
): Promise<AgentSession>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams & { oneShot?: undefined },
  handler: acp.Client | undefined,
): Promise<AgentSession>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams,
): Promise<AgentSession | null>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams,
  handler: acp.Client | undefined,
): Promise<AgentSession | null>

/** Starts or attaches to a daemon-backed ACP agent session using one already-bound daemon client. */
export async function runSession(
  client: DaemonIpcClient,
  params: SessionParams,
  handler?: acp.Client,
): Promise<AgentSession | null> {
  const connectedSession =
    "sessionId" in params && params.sessionId !== undefined
      ? await client.send("sessionConnect", { id: params.sessionId })
      : await client.send("sessionCreate", {
          agent: params.agent,
          cwd: params.cwd,
          worktree: params.worktree,
          workforce: params.workforce,
          mcpServers: params.mcpServers,
          systemPrompt: params.systemPrompt ?? "",
          env: params.env,
          repository: params.repository,
          prNumber: params.prNumber,
          metadata: params.metadata,
          initialPrompt: params.initialPrompt,
          oneShot: params.oneShot,
        })

  if (shouldExitAfterInitialPrompt(params)) {
    return null
  }

  const daemonSessionId = connectedSession.session.id
  const acpSessionId = connectedSession.session.acpSessionId

  const agentInput = createMessageInputTransport(client, daemonSessionId)
  const agentOutput = await createMessageOutputTransport(client, daemonSessionId)

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
    acp.ndJsonStream(agentInput, agentOutput.readable),
  )

  return new AgentSession(daemonSessionId, acpSessionId, acpClient, client, agentOutput.close)
}
