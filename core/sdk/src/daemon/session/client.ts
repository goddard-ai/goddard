import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type { DaemonSession } from "@goddard-ai/session/schema"
import * as acp from "acp-client/protocol"

import type { SessionParams } from "../../session.ts"
import { AgentSession, type AgentSessionAcpClient } from "./client-session.ts"

type CreateRunSessionParams = Extract<SessionParams, { sessionId?: undefined }>

type PendingAcpRequest = {
  resolve: (result: unknown) => void
  reject: (error: Error) => void
}

/** Detects the session-creation case that returns no live client session object. */
function shouldExitAfterInitialPrompt(params: SessionParams): boolean {
  return isNewSessionParams(params) && params.oneShot === true
}

function isNewSessionParams(params: SessionParams): params is CreateRunSessionParams {
  return !("sessionId" in params) || params.sessionId === undefined
}

/** Sends agent requests and dispatches client-side callbacks over one ACP stream. */
class DaemonBackedAcpClient implements AgentSessionAcpClient {
  private nextRequestId = 0
  private readonly pendingRequests = new Map<acp.RequestId, PendingAcpRequest>()
  private readonly writer: WritableStreamDefaultWriter<acp.AnyMessage>
  private readonly handler: acp.Client

  constructor(handler: acp.Client, stream: acp.Stream) {
    this.handler = handler
    this.writer = stream.writable.getWriter()
    void this.readMessages(stream.readable)
  }

  prompt(params: acp.PromptRequest) {
    return this.sendRequest<acp.PromptResponse>(acp.AGENT_METHODS.session_prompt, params)
  }

  private sendRequest<T>(method: string, params: unknown) {
    const id = `goddard-sdk-${++this.nextRequestId}`

    return new Promise<T>((resolve, reject) => {
      this.pendingRequests.set(id, {
        resolve: (result) => resolve(result as T),
        reject,
      })

      void this.writer
        .write({
          jsonrpc: "2.0",
          id,
          method,
          params,
        })
        .catch((error) => {
          this.pendingRequests.delete(id)
          reject(toError(error))
        })
    })
  }

  private async readMessages(readable: ReadableStream<acp.AnyMessage>) {
    const reader = readable.getReader()

    try {
      while (true) {
        const { done, value } = await reader.read()
        if (done) {
          this.rejectPendingRequests(new Error("ACP stream closed"))
          return
        }

        await this.handleMessage(value)
      }
    } catch (error) {
      this.rejectPendingRequests(toError(error))
    } finally {
      reader.releaseLock()
    }
  }

  private async handleMessage(message: acp.AnyMessage) {
    if (isAcpResponse(message)) {
      this.resolveResponse(message)
      return
    }

    if (!isAcpRequestOrNotification(message)) {
      return
    }

    try {
      const result = await this.dispatchClientMessage(message.method, message.params)
      if ("id" in message) {
        await this.writer.write({
          jsonrpc: "2.0",
          id: message.id,
          result,
        })
      }
    } catch (error) {
      if ("id" in message) {
        await this.writer.write({
          jsonrpc: "2.0",
          id: message.id,
          error: {
            code: -32603,
            message: toError(error).message,
          },
        })
      }
    }
  }

  private resolveResponse(message: acp.AnyMessage & { id: acp.RequestId }) {
    const pendingRequest = this.pendingRequests.get(message.id)
    if (!pendingRequest) {
      return
    }

    this.pendingRequests.delete(message.id)

    if ("error" in message) {
      pendingRequest.reject(createAcpResponseError(message.error))
      return
    }

    pendingRequest.resolve("result" in message ? message.result : undefined)
  }

  private async dispatchClientMessage(method: string, params: unknown) {
    switch (method) {
      case acp.CLIENT_METHODS.session_update:
        await this.handler.sessionUpdate(params as acp.SessionNotification)
        return null
      case acp.CLIENT_METHODS.session_request_permission:
        return await this.handler.requestPermission(params as acp.RequestPermissionRequest)
      case acp.CLIENT_METHODS.fs_read_text_file:
        return await callOptionalClientHandler(this.handler.readTextFile, params)
      case acp.CLIENT_METHODS.fs_write_text_file:
        return await callOptionalClientHandler(this.handler.writeTextFile, params)
      case acp.CLIENT_METHODS.terminal_create:
        return await callOptionalClientHandler(this.handler.createTerminal, params)
      case acp.CLIENT_METHODS.terminal_output:
        return await callOptionalClientHandler(this.handler.terminalOutput, params)
      case acp.CLIENT_METHODS.terminal_release:
        return await callOptionalClientHandler(this.handler.releaseTerminal, params)
      case acp.CLIENT_METHODS.terminal_wait_for_exit:
        return await callOptionalClientHandler(this.handler.waitForTerminalExit, params)
      case acp.CLIENT_METHODS.terminal_kill:
        return await callOptionalClientHandler(this.handler.killTerminal, params)
      case acp.CLIENT_METHODS.elicitation_create:
        return await callOptionalClientHandler(this.handler.unstable_createElicitation, params)
      case acp.CLIENT_METHODS.elicitation_complete:
        return await callOptionalClientHandler(this.handler.unstable_completeElicitation, params)
      default:
        if (method.startsWith("_")) {
          if (this.handler.extMethod) {
            return await this.handler.extMethod(method, toRecord(params))
          }
          if (this.handler.extNotification) {
            await this.handler.extNotification(method, toRecord(params))
            return null
          }
        }
        throw new Error(`Unsupported ACP client method: ${method}`)
    }
  }

  private rejectPendingRequests(error: Error) {
    for (const pendingRequest of this.pendingRequests.values()) {
      pendingRequest.reject(error)
    }
    this.pendingRequests.clear()
  }
}

/** Calls an optional ACP client handler or returns the standard method-missing failure. */
async function callOptionalClientHandler(
  handler: ((params: never) => Promise<unknown>) | undefined,
  params: unknown,
) {
  if (!handler) {
    throw new Error("Unsupported ACP client method")
  }

  return await handler(params as never)
}

/** Returns true when one ACP message is a response to a request sent by this client. */
function isAcpResponse(message: acp.AnyMessage): message is acp.AnyMessage & { id: acp.RequestId } {
  return (
    typeof message === "object" &&
    message !== null &&
    "id" in message &&
    ("result" in message || "error" in message)
  )
}

/** Returns true when one ACP message carries a client method to dispatch. */
function isAcpRequestOrNotification(
  message: acp.AnyMessage,
): message is acp.AnyMessage & { method: string; params?: unknown } {
  return (
    typeof message === "object" &&
    message !== null &&
    "method" in message &&
    typeof message.method === "string"
  )
}

/** Normalizes unknown extension params to the object shape ACP extension handlers expect. */
function toRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}
}

/** Converts thrown or rejected values into Error instances. */
function toError(error: unknown) {
  return error instanceof Error ? error : new Error(String(error))
}

/** Preserves useful JSON-RPC error messages when an ACP request fails. */
function createAcpResponseError(error: unknown) {
  if (typeof error === "object" && error !== null && "message" in error) {
    return new Error(String(error.message))
  }

  return toError(error)
}

/** Turns a writable ACP transport into daemon `session.send` requests. */
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

      await client.session.send({
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

    await client.session.send({
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
  const abortController = new AbortController()

  const events = await client.session.streamMessages(
    { id },
    {
      signal: abortController.signal,
    },
  )
  const done = (async () => {
    for await (const message of events) {
      if (closed) {
        return
      }

      if (
        typeof message === "object" &&
        message !== null &&
        "method" in message &&
        typeof message.method === "string" &&
        agentMethods.has(message.method)
      ) {
        // The daemon stream includes echoed agent-bound requests; the SDK bridge only forwards
        // agent responses and client-bound requests back into acp-client's readable side.
        continue
      }

      void writer.write(encoder.encode(`${JSON.stringify(message)}\n`)).catch(() => {})
    }
  })()

  return {
    readable: stream.readable,
    close: async () => {
      if (closed) {
        return
      }

      closed = true
      abortController.abort()
      await done.catch(() => {})
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
      ? await client.session.connect({ id: params.sessionId })
      : await client.session.create({
          agent: params.agent,
          cwd: params.cwd,
          launchLeaseId: params.launchLeaseId,
          localCheckout: params.localCheckout,
          worktree: params.worktree,
          mcpServers: params.mcpServers,
          systemPrompt: params.systemPrompt,
          initialModelId: params.initialModelId,
          initialConfigOptions: params.initialConfigOptions,
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

  const acpClient = new DaemonBackedAcpClient(
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
