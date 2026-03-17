import * as acp from "@agentclientprotocol/sdk"
import type { SessionStatus } from "@goddard-ai/schema/db"
import type { AgentDistribution, SessionParams } from "@goddard-ai/schema/session-server"
import { SessionStorage, SQLSessionUpdate } from "@goddard-ai/storage"
import { spawn } from "node:child_process"
import { Readable, Writable } from "node:stream"
import { noop, once } from "radashi"
import { serve } from "srvx"
import { createAgentConnection, getAcpMessageResult, isAcpRequest, matchAcpRequest } from "./acp.js"
import { createWebSocketHandler } from "./node/websocket-server.js"
import { fetchRegistryAgent } from "./registry.js"

/** The current version of `@goddard-ai/session` */
declare const __VERSION__: string

export function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest {
  return {
    ...request,
    prompt: [
      { type: "text", text: `<system-prompt>${systemPrompt}</system-prompt>` },
      ...request.prompt,
    ],
  }
}

export function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: SessionStatus,
): SessionStatus | null {
  if (status === "active") {
    const cancelRequest = matchAcpRequest<acp.CancelRequestNotification>(
      message,
      acp.AGENT_METHODS.session_cancel,
    )
    if (cancelRequest) {
      return "cancelled"
    }
  }
  return null
}

export function sessionStatusFromAgentMessage(
  clientRequest: acp.AnyMessage | undefined,
  message: acp.AnyMessage,
): SessionStatus | null {
  const promptRequest = clientRequest
    ? matchAcpRequest<acp.PromptRequest>(clientRequest, acp.AGENT_METHODS.session_prompt)
    : null
  if (promptRequest) {
    const result = getAcpMessageResult<acp.PromptResponse>(message)
    if (result?.stopReason === "end_turn") {
      return "done"
    }
  }
  return null
}

export function shouldExitAfterInitialPrompt(
  params: SessionParams,
): params is SessionParams & { oneShot: true } {
  return !isPropertyDefined(params, "sessionId") && params.oneShot === true
}

export function buildAgentProcessEnv(
  serverId: string,
  extraEnv?: Record<string, string>,
): NodeJS.ProcessEnv {
  return {
    ...process.env,
    ...extraEnv,
    PATH: extraEnv?.PATH ?? process.env.PATH ?? "",
    GODDARD_SERVER_ID: serverId,
  }
}

/**
 * Resolve the agent executable and spawn the agent subprocess based on the
 * provided configuration. No messages are sent to the subprocess at this stage.
 */
async function spawnAgentProcess(
  serverId: string,
  params: {
    agent: string | AgentDistribution
    sessionId?: string
    env?: Record<string, string>
  },
) {
  let agent = params.agent
  if (typeof agent === "string") {
    const fetchedAgent = await fetchRegistryAgent(agent)
    if (!fetchedAgent) {
      throw new Error(`Agent not found: ${agent}`)
    }
    agent = fetchedAgent.distribution
  }

  let cmd: string
  let args: string[]

  if (agent.type === "npx" && agent.package) {
    cmd = "npx"
    args = ["-y", agent.package]
  } else if (agent.type === "binary" && agent.cmd) {
    cmd = agent.cmd
    args = agent.args || []
  } else if (agent.type === "uvx" && agent.package) {
    cmd = "uvx"
    args = [agent.package]
  } else {
    throw new Error("Unsupported agent distribution")
  }

  return spawn(cmd, args, {
    stdio: ["pipe", "pipe", "inherit"],
    env: buildAgentProcessEnv(serverId, params.env),
  })
}

function isPropertyDefined<T extends object, P extends T extends any ? keyof T : never>(
  obj: T,
  property: P,
): obj is T & Required<Pick<T, P>> {
  return property in obj && obj[property] !== undefined
}

async function initializeSession(input: Writable, output: Readable, params: SessionParams) {
  const history: acp.AnyMessage[] = []
  const stream = acp.ndJsonStream(
    Writable.toWeb(input),
    Readable.toWeb(output) as ReadableStream<Uint8Array>,
  )

  const agent = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        // Permission requests are not expected during initialization.
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params) {
        // Capture historical messages while `session/load` initializes.
        history.push({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params,
        } satisfies acp.AnyMessage)
      },
    }),
    stream,
  )

  try {
    const handshake = await agent.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: { name: "npm:@goddard-ai/session", version: __VERSION__ },
    })

    const session = {
      status: "active" as SessionStatus,
      isFirstPrompt: true,
      lastPermissionRequest: null as AcpPermissionRequest | null,
      history,
      handshake,
    }

    if (isPropertyDefined(params, "sessionId")) {
      if (!handshake.agentCapabilities?.loadSession) {
        throw new Error("Agent does not support loading existing sessions")
      }
      const loadedSession = await agent.loadSession(params)
      return {
        ...session,
        ...loadedSession,
        sessionId: params.sessionId,
      }
    }

    const newSession = await agent.newSession(params)
    if (isPropertyDefined(params, "initialPrompt")) {
      const prompt = params.initialPrompt

      let promptRequest: acp.PromptRequest = {
        sessionId: newSession.sessionId,
        prompt: typeof prompt === "string" ? [{ type: "text", text: prompt }] : prompt,
      }
      promptRequest = injectSystemPrompt(promptRequest, params.systemPrompt)

      history.push({
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_prompt,
        params: promptRequest,
      } satisfies acp.AnyMessage)

      const promptResponse = await agent.prompt(promptRequest)
      if (promptResponse.stopReason === "end_turn") {
        session.status = "done"
      }
      session.isFirstPrompt = false
    }
    return {
      ...session,
      ...newSession,
    }
  } finally {
    await stream.readable.cancel()
    await stream.writable.close()
  }
}

type AcpRequestMap = Map<string | number, acp.AnyMessage & { method: string }>

type AcpPermissionRequest = acp.AnyMessage & {
  id: unknown
  params: acp.RequestPermissionRequest
}

type AcpPromptRequest = acp.AnyMessage & {
  params: acp.PromptRequest
}

export async function serveAgent(serverId: string, params: SessionParams) {
  const agentProcess = await spawnAgentProcess(serverId, params)
  const session = await initializeSession(agentProcess.stdin, agentProcess.stdout, params)

  const updateSession = async (update: SQLSessionUpdate) => {
    if (update.status) {
      session.status = update.status
    }
    await SessionStorage.update(session.sessionId, update).catch(noop)
  }

  const storeSession = once(async (serverURL: URL | null, status: SessionStatus) => {
    const sessionUpdate = {
      serverId,
      serverAddress: serverURL?.href ?? null,
      serverPid: serverURL ? process.pid : null,
      agentName:
        typeof params.agent === "string"
          ? params.agent
          : (params.agent.package ?? params.agent.cmd ?? "custom"),
      status,
      cwd: params.cwd,
      mcpServers: params.mcpServers,
      metadata: params.metadata ?? null,
    }

    if (params.sessionId !== undefined) {
      await updateSession(sessionUpdate)
    } else {
      await SessionStorage.create({
        id: session.sessionId,
        ...sessionUpdate,
      })
    }
  })

  if (shouldExitAfterInitialPrompt(params)) {
    await storeSession(null, "done")
    agentProcess.kill()
    return {
      serverAddress: null,
      sessionId: session.sessionId,
    }
  }

  const clientRequests: AcpRequestMap = new Map()

  const agentConnection = createAgentConnection(agentProcess.stdin, agentProcess.stdout)
  const agentInput = agentConnection.getWriter()

  const server = serve({
    hostname: "localhost",
    port: Number(process.env.PORT || 0),
    silent: true,
    fetch: async (request) => {
      const url = new URL(request.url)

      if (request.method === "GET" && url.pathname === "/history") {
        return Response.json(session.history)
      }

      if (request.method === "POST" && url.pathname === "/shutdown") {
        agentProcess.kill()
        return Response.json({ ok: true })
      }

      return Response.json({ error: "Not Found" }, { status: 404 })
    },
  })

  const wss = createWebSocketHandler({
    onConnection(ws) {
      if (session.lastPermissionRequest) {
        ws.send(JSON.stringify(session.lastPermissionRequest))
      }
    },
    async onMessage(message: acp.AnyMessage, ws) {
      // Any client can respond to permission requests, so clear the
      // tracked request on any response.
      if (
        session.lastPermissionRequest &&
        "id" in message &&
        message.id === session.lastPermissionRequest.id
      ) {
        session.lastPermissionRequest = null
      } else {
        // Infer session status from client messages if possible.
        const nextStatus = sessionStatusFromClientMessage(message, session.status)
        if (nextStatus) {
          updateSession({ status: nextStatus }).catch(noop)
        }

        if (
          session.isFirstPrompt &&
          isAcpRequest<AcpPromptRequest>(message, acp.AGENT_METHODS.session_prompt)
        ) {
          session.isFirstPrompt = false
          message.params = injectSystemPrompt(message.params, params.systemPrompt)
        }
      }

      session.history.push(message)
      wss.broadcast(message, { exclude: ws })
      await agentInput.write(message)
    },
  })

  const agentSubscription = agentConnection.subscribe(async (message) => {
    // Track permission requests, so any client can respond.
    if (
      isAcpRequest<AcpPermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)
    ) {
      session.lastPermissionRequest = message
    }
    // Infer session status from agent responses.
    else if ("id" in message && message.id != null) {
      const clientRequest = clientRequests.get(message.id)
      const nextStatus = sessionStatusFromAgentMessage(clientRequest, message)
      if (nextStatus) {
        updateSession({ status: nextStatus }).catch(noop)
      }
      if (clientRequest) {
        clientRequests.delete(message.id)
      }
    }

    session.history.push(message)
    wss.broadcast(message)
  })

  agentProcess.once("exit", async () => {
    wss.close()
    await server.close(true).catch(noop)
    await agentInput.close().catch(noop)
    await agentSubscription.close().catch(noop)
  })

  wss.listen(server, "/acp")
  await server.ready()

  const serverAddress = new URL(server.url!)
  await storeSession(serverAddress, session.status)

  agentProcess.once("exit", async (code, signal) => {
    const nextUpdate: SQLSessionUpdate = {
      serverAddress: null,
      serverPid: null,
    }
    if (code !== 0 && code !== null) {
      nextUpdate.status = "error"
      nextUpdate.errorMessage = `Exited with code ${code}`
    } else if (isErrorSignal(signal)) {
      nextUpdate.status = "error"
      nextUpdate.errorMessage = `Killed by ${signal}`
    } else if (session.status !== "done") {
      nextUpdate.status = "cancelled"
    }
    await updateSession(nextUpdate).catch(noop)
  })

  return {
    serverAddress,
    sessionId: session.sessionId,
  }
}

function isErrorSignal(signal: string | null): boolean {
  return signal === "SIGKILL" || signal === "SIGABRT" || signal === "SIGQUIT"
}
