import * as acp from "@agentclientprotocol/sdk"
import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import { createDaemonIpcClient, createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client"
import type { SessionParams } from "@goddard-ai/schema/session-server"
import { AgentSession } from "./client-session.js"

// Configuration used to connect SDK session helpers to a daemon IPC client.
export type RunAgentOptions = {
  client?: DaemonIpcClient
  daemonUrl?: string
  createClient?: (input: { socketPath: string }) => DaemonIpcClient
  env?: Record<string, string | undefined>
}

function shouldExitAfterInitialPrompt(params: SessionParams): boolean {
  return "sessionId" in params === false && params.oneShot === true
}

function createMessageInputTransport(client: DaemonIpcClient, id: string): WritableStream {
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

async function flushMessageBuffer(
  buffer: string,
  client: DaemonIpcClient,
  id: string,
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

function decodeStreamChunk(chunk: unknown, decoder: TextDecoder): string {
  if (typeof chunk === "string") {
    return chunk
  }
  if (chunk instanceof Uint8Array) {
    return decoder.decode(chunk, { stream: true })
  }

  throw new Error(`Unsupported ACP transport chunk: ${String(chunk)}`)
}

async function createMessageOutputTransport(client: DaemonIpcClient, id: string): Promise<{
  readable: ReadableStream<Uint8Array>
  close: () => Promise<void>
}> {
  const agentMethods = new Set<string>(Object.values(acp.AGENT_METHODS))
  const encoder = new TextEncoder()
  const stream = new TransformStream<Uint8Array, Uint8Array>()
  const writer = stream.writable.getWriter()
  let closed = false

  const unsubscribe = await client.subscribe("sessionMessage", ({ id: messageId, message }) => {
    if (
      messageId !== id ||
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
  })

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

function resolveDaemonClient(options?: RunAgentOptions): DaemonIpcClient {
  if (options?.client) {
    return options.client
  }

  if (options?.daemonUrl) {
    return createDaemonIpcClient({
      daemonUrl: options.daemonUrl,
      createClient: options.createClient,
    })
  }

  return createDaemonIpcClientFromEnv({
    env: options?.env,
    createClient: options?.createClient,
  }).client
}

export async function runAgent(
  params: SessionParams & { oneShot: true },
  handler?: acp.Client,
  options?: RunAgentOptions,
): Promise<null>

export async function runAgent(
  params: SessionParams & { oneShot?: undefined },
  handler?: acp.Client,
  options?: RunAgentOptions,
): Promise<AgentSession>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
  options?: RunAgentOptions,
): Promise<AgentSession | null>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
  options?: RunAgentOptions,
): Promise<AgentSession | null> {
  const client = resolveDaemonClient(options)

  const connectedSession =
    "sessionId" in params && params.sessionId !== undefined
      ? await client.send("sessionConnect", { id: params.sessionId })
      : await client.send("sessionCreate", {
          agent: params.agent,
          cwd: params.cwd,
          mcpServers: params.mcpServers,
          systemPrompt: params.systemPrompt ?? "",
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
    acp.ndJsonStream(
      agentInput,
      agentOutput.readable,
    ),
  )

  return new AgentSession(daemonSessionId, acpSessionId, acpClient, client, agentOutput.close)
}
