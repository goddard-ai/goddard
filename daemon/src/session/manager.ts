import * as acp from "@agentclientprotocol/sdk"
import type {
  CreateDaemonSessionRequest,
  DaemonDiagnosticEvent,
  DaemonSession,
  DaemonSessionConnection,
  DaemonSessionMetadata,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
} from "@goddard-ai/schema/daemon"
import type { SessionStatus } from "@goddard-ai/schema/db"
import type { AgentDistribution } from "@goddard-ai/schema/session-server"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import {
  SessionStateStorage,
  SessionStorage,
  type SessionConnectionMode,
  type SessionDiagnosticEvent,
  type SQLSessionUpdate,
} from "@goddard-ai/storage"
import { randomUUID, randomBytes } from "node:crypto"
import { spawn, type ChildProcessByStdio } from "node:child_process"
import { Readable, Writable } from "node:stream"
import { createAgentConnection, getAcpMessageResult, isAcpRequest, matchAcpRequest } from "./acp.ts"
import { prependAgentBinToPath } from "../config.ts"
import { fetchRegistryAgent } from "./registry.ts"

/** The current version of `@goddard-ai/daemon` */
declare const __VERSION__: string

function getPackageVersion(): string {
  try {
    return __VERSION__
  } catch {
    return "0.0.0"
  }
}

type ClientRequestMap = Map<string | number, acp.AnyMessage & { method: string }>

type PermissionRequest = acp.AnyMessage & {
  id: unknown
  params: acp.RequestPermissionRequest
}

// ACP request metadata tracked in memory so response messages can update session state.
type PromptRequestMessage = acp.AnyMessage & {
  params: acp.PromptRequest
}

// Live daemon-owned session runtime that cannot survive process restarts.
type ActiveSession = {
  id: string
  acpId: string
  token: string
  process: ChildProcessByStdio<Writable, Readable, null>
  writer: WritableStreamDefaultWriter<acp.AnyMessage>
  subscription: {
    close: () => Promise<void>
  }
  status: SessionStatus
  history: acp.AnyMessage[]
  isFirstPrompt: boolean
  systemPrompt: string
  lastPermissionRequest: PermissionRequest | null
  clientRequests: ClientRequestMap
}

export type SessionManager = {
  createSession: (params: CreateDaemonSessionRequest) => Promise<DaemonSession>
  connectSession: (id: string) => Promise<DaemonSession>
  getSession: (id: string) => Promise<DaemonSession>
  getHistory: (id: string) => Promise<GetDaemonSessionHistoryResponse>
  getDiagnostics: (id: string) => Promise<GetDaemonSessionDiagnosticsResponse>
  sendMessage: (id: string, message: acp.AnyMessage) => Promise<void>
  shutdownSession: (id: string) => Promise<boolean>
  resolveSessionIdByToken: (token: string) => Promise<string>
  close: () => Promise<void>
}

export function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest {
  return {
    ...request,
    prompt: [
      { type: "text", text: `<system-prompt name="Goddard CLI">${systemPrompt}</system-prompt>` },
      ...request.prompt,
    ],
  }
}

function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: SessionStatus,
): SessionStatus | null {
  if (status !== "active") {
    return null
  }

  const cancelRequest = matchAcpRequest<acp.CancelRequestNotification>(
    message,
    acp.AGENT_METHODS.session_cancel,
  )
  if (cancelRequest) {
    return "cancelled"
  }

  return null
}

function sessionStatusFromAgentMessage(
  clientRequest: acp.AnyMessage | undefined,
  message: acp.AnyMessage,
): SessionStatus | null {
  const promptRequest = clientRequest
    ? matchAcpRequest<acp.PromptRequest>(clientRequest, acp.AGENT_METHODS.session_prompt)
    : null

  if (!promptRequest) {
    return null
  }

  const result = getAcpMessageResult<acp.PromptResponse>(message)
  if (result?.stopReason === "end_turn") {
    return "done"
  }

  return null
}

function isErrorSignal(signal: string | null): boolean {
  return signal === "SIGKILL" || signal === "SIGABRT" || signal === "SIGQUIT"
}

function shouldExitAfterInitialPrompt(params: CreateDaemonSessionRequest): boolean {
  return params.oneShot === true && params.initialPrompt !== undefined
}

function toIsoString(value: Date | number): string {
  return (value instanceof Date ? value : new Date(value)).toISOString()
}

function toConnectionState(input: {
  mode: SessionConnectionMode
  activeDaemonSession: boolean
  historyLength: number
}): DaemonSessionConnection {
  return {
    mode: input.mode,
    reconnectable: input.mode === "live" && input.activeDaemonSession,
    historyAvailable: input.historyLength > 0,
    activeDaemonSession: input.activeDaemonSession,
  }
}

async function toDaemonSession(
  record: Awaited<ReturnType<typeof SessionStorage.get>>,
): Promise<DaemonSession> {
  if (!record) {
    throw new Error("Session not found")
  }

  const state = await SessionStateStorage.get(record.id)
  const diagnostics = state?.diagnostics ?? []
  const historyLength = state?.history.length ?? 0

  return {
    id: record.id,
    acpId: record.acpId,
    status: record.status,
    agentName: record.agentName,
    cwd: record.cwd,
    metadata: record.metadata,
    connection: toConnectionState({
      mode: state?.connectionMode ?? "none",
      activeDaemonSession: state?.activeDaemonSession ?? false,
      historyLength,
    }),
    diagnostics: {
      eventCount: diagnostics.length,
      historyLength,
      lastEventAt: diagnostics.at(-1)?.at ?? null,
    },
    createdAt: toIsoString(record.createdAt),
    updatedAt: toIsoString(record.updatedAt),
    errorMessage: record.errorMessage,
    blockedReason: record.blockedReason,
    initiative: record.initiative,
    lastAgentMessage: record.lastAgentMessage,
  }
}

function agentNameFromInput(agent: string | AgentDistribution): string {
  if (typeof agent === "string") {
    return agent
  }

  return agent.package ?? agent.cmd ?? "custom"
}

function buildAgentProcessEnv(input: {
  daemonUrl: string
  token: string
  agentBinDir: string
  env?: Record<string, string>
}): NodeJS.ProcessEnv {
  return {
    ...process.env,
    ...prependAgentBinToPath(input.agentBinDir, input.env),
    GODDARD_DAEMON_URL: input.daemonUrl,
    GODDARD_SESSION_TOKEN: input.token,
  }
}

async function spawnAgentProcess(
  daemonUrl: string,
  token: string,
  params: {
    agent: string | AgentDistribution
    cwd: string
    agentBinDir: string
    env?: Record<string, string>
  },
): Promise<ChildProcessByStdio<Writable, Readable, null>> {
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
    cwd: params.cwd,
    env: buildAgentProcessEnv({
      daemonUrl,
      token,
      agentBinDir: params.agentBinDir,
      env: params.env,
    }),
  })
}

async function initializeSession(
  input: Writable,
  output: Readable,
  params: CreateDaemonSessionRequest,
): Promise<{
  status: SessionStatus
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  acpId: string
}> {
  const history: acp.AnyMessage[] = []
  const stream = acp.ndJsonStream(
    Writable.toWeb(input),
    Readable.toWeb(output) as ReadableStream<Uint8Array>,
  )

  const agent = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params: any) {
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
    await agent.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: { name: "npm:@goddard-ai/daemon", version: getPackageVersion() },
    })

    const newSession = await agent.newSession(params)
    let status: SessionStatus = "active"
    let isFirstPrompt = true

    if (params.initialPrompt !== undefined) {
      const promptRequest = injectSystemPrompt(
        {
          sessionId: newSession.sessionId,
          prompt:
            typeof params.initialPrompt === "string"
              ? [{ type: "text", text: params.initialPrompt }]
              : params.initialPrompt,
        },
        params.systemPrompt,
      )

      history.push({
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_prompt,
        params: promptRequest,
      } satisfies acp.AnyMessage)

      const response = await agent.prompt(promptRequest)
      if (response.stopReason === "end_turn") {
        status = "done"
      }
      isFirstPrompt = false
    }

    return {
      status,
      isFirstPrompt,
      history,
      acpId: newSession.sessionId,
    }
  } finally {
    await stream.readable.cancel().catch(() => {})
    await stream.writable.close().catch(() => {})
  }
}

function parseRepoScope(metadata?: DaemonSessionMetadata): {
  owner: string
  repo: string
  allowedPrNumbers: number[]
} {
  const repository = metadata?.repository?.trim() ?? ""
  const [owner, repo] = repository.split("/")

  return {
    owner: owner ?? "",
    repo: repo ?? "",
    allowedPrNumbers: typeof metadata?.prNumber === "number" ? [metadata.prNumber] : [],
  }
}

function createDiagnosticEvent(
  sessionId: string,
  type: string,
  detail?: Record<string, unknown>,
): SessionDiagnosticEvent {
  return {
    sessionId,
    type,
    at: new Date().toISOString(),
    detail,
  }
}

export function createSessionManager(input: {
  daemonUrl: string
  agentBinDir: string
  publish: (id: string, message: acp.AnyMessage) => void
}): SessionManager {
  const activeSessions = new Map<string, ActiveSession>()
  const ready = reconcilePersistedSessions()

  async function updateSession(id: string, update: SQLSessionUpdate): Promise<void> {
    const active = activeSessions.get(id)
    if (update.status && active) {
      active.status = update.status
    }

    await SessionStorage.update(id, update)
  }

  async function appendHistory(id: string, message: acp.AnyMessage): Promise<void> {
    const active = activeSessions.get(id)
    if (active) {
      active.history.push(message)
    }
    await SessionStateStorage.appendHistory(id, message)
  }

  async function emitDiagnostic(
    sessionId: string,
    type: string,
    detail?: Record<string, unknown>,
  ): Promise<void> {
    const event = createDiagnosticEvent(sessionId, type, detail)
    process.stdout.write(`${JSON.stringify({ scope: "daemon", ...event })}\n`)
    await SessionStateStorage.appendDiagnostic(sessionId, event)
  }

  async function setConnectionMode(
    sessionId: string,
    mode: SessionConnectionMode,
    activeDaemonSession: boolean,
  ): Promise<void> {
    await SessionStateStorage.update(sessionId, {
      connectionMode: mode,
      activeDaemonSession,
    })
  }

  async function reconcilePersistedSessions(): Promise<void> {
    let persistedSessions: Awaited<ReturnType<typeof SessionStorage.list>>
    let permissions: Awaited<ReturnType<typeof SessionPermissionsStorage.list>>

    try {
      ;[persistedSessions, permissions] = await Promise.all([
        SessionStorage.list(),
        SessionPermissionsStorage.list(),
      ])
    } catch (error) {
      process.stderr.write(
        `${JSON.stringify({
          scope: "daemon",
          type: "session_reconciliation_failed",
          at: new Date().toISOString(),
          detail: { message: error instanceof Error ? error.message : String(error) },
        })}\n`,
      )
      return
    }

    const sessionIds = new Set(persistedSessions.map((session) => session.id))

    await Promise.all(
      permissions
        .filter((permission) => !sessionIds.has(permission.sessionId))
        .map((permission) => SessionPermissionsStorage.revoke(permission.sessionId)),
    )

    await Promise.all(
      persistedSessions.map(async (session) => {
        const state = await SessionStateStorage.get(session.id)
        if (!state) {
          await SessionStateStorage.create({
            sessionId: session.id,
            acpId: session.acpId,
            connectionMode: "none",
            history: [],
            diagnostics: [],
            activeDaemonSession: false,
          })
        }

        if (
          session.status === "active" ||
          session.status === "blocked" ||
          session.status === "idle"
        ) {
          await SessionStorage.update(session.id, {
            status: "error",
            errorMessage: "Session interrupted when the previous daemon exited unexpectedly.",
          })
          await setConnectionMode(session.id, "history", false)
          await emitDiagnostic(session.id, "session_reconciled_after_restart", {
            previousStatus: session.status,
          })
          await SessionPermissionsStorage.revoke(session.id).catch(() => {})
          return
        }

        await setConnectionMode(session.id, state?.history.length ? "history" : "none", false)
        await SessionPermissionsStorage.revoke(session.id).catch(() => {})
      }),
    )
  }

  async function createSession(params: CreateDaemonSessionRequest): Promise<DaemonSession> {
    await ready
    const id = randomUUID()
    const token = randomBytes(32).toString("hex")
    const scope = parseRepoScope(params.metadata)

    await SessionPermissionsStorage.create({
      sessionId: id,
      token,
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    })

    try {
      const process = await spawnAgentProcess(input.daemonUrl, token, {
        agent: params.agent,
        cwd: params.cwd,
        agentBinDir: input.agentBinDir,
        env: params.env,
      })

      const initialized = await initializeSession(process.stdin, process.stdout, params)

      await SessionStateStorage.create({
        sessionId: id,
        acpId: initialized.acpId,
        connectionMode: shouldExitAfterInitialPrompt(params) ? "history" : "live",
        history: [...initialized.history],
        diagnostics: [],
        activeDaemonSession: !shouldExitAfterInitialPrompt(params),
      })

      await SessionStorage.create({
        id,
        acpId: initialized.acpId,
        status: initialized.status,
        agentName: agentNameFromInput(params.agent),
        cwd: params.cwd,
        mcpServers: params.mcpServers,
        metadata: params.metadata ?? null,
      })
      await emitDiagnostic(id, "session_created", {
        status: initialized.status,
        oneShot: params.oneShot === true,
        agent: agentNameFromInput(params.agent),
      })

      if (shouldExitAfterInitialPrompt(params)) {
        await updateSession(id, { status: "done" })
        await setConnectionMode(id, "history", false)
        await emitDiagnostic(id, "session_completed_one_shot")
        process.kill()
        await SessionPermissionsStorage.revoke(id).catch(() => {})
        return toDaemonSession(await SessionStorage.get(id))
      }

      const connection = createAgentConnection(process.stdin, process.stdout)
      const writer = connection.getWriter()
      const active: ActiveSession = {
        id,
        acpId: initialized.acpId,
        token,
        process,
        writer,
        subscription: { close: async () => {} },
        status: initialized.status,
        history: [...initialized.history],
        isFirstPrompt: initialized.isFirstPrompt,
        systemPrompt: params.systemPrompt,
        lastPermissionRequest: null,
        clientRequests: new Map(),
      }

      active.subscription = connection.subscribe(async (message) => {
        if (
          isAcpRequest<PermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)
        ) {
          active.lastPermissionRequest = message
        } else if ("id" in message && message.id != null) {
          const clientRequest = active.clientRequests.get(message.id)
          const nextStatus = sessionStatusFromAgentMessage(clientRequest, message)
          if (nextStatus) {
            await updateSession(active.id, { status: nextStatus })
          }
          if (clientRequest) {
            active.clientRequests.delete(message.id)
          }
        }

        await appendHistory(active.id, message)
        input.publish(active.id, message)
      })

      const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
        activeSessions.delete(active.id)
        await active.writer.close().catch(() => {})
        await active.subscription.close().catch(() => {})
        await SessionPermissionsStorage.revoke(active.id).catch(() => {})

        const nextUpdate: SQLSessionUpdate = {}
        if (code !== 0 && code !== null) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Exited with code ${code}`
        } else if (isErrorSignal(signal)) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Killed by ${signal}`
        } else if (active.status !== "done") {
          nextUpdate.status = "cancelled"
        }

        await setConnectionMode(active.id, active.history.length > 0 ? "history" : "none", false)
        await emitDiagnostic(active.id, "agent_process_exit", {
          code,
          signal,
          nextStatus: nextUpdate.status ?? active.status,
        })
        if (Object.keys(nextUpdate).length > 0) {
          await updateSession(active.id, nextUpdate).catch(() => {})
        }
      }

      process.once("exit", (code, signal) => {
        void handleExit(code, signal)
      })

      activeSessions.set(active.id, active)
      return toDaemonSession(await SessionStorage.get(id))
    } catch (error) {
      await SessionPermissionsStorage.revoke(id).catch(() => {})
      await SessionStateStorage.remove(id).catch(() => {})
      throw error
    }
  }

  async function getSession(id: string): Promise<DaemonSession> {
    await ready
    return toDaemonSession(await SessionStorage.get(id))
  }

  async function connectSession(id: string): Promise<DaemonSession> {
    await ready
    if (!activeSessions.has(id)) {
      const session = await getSession(id)
      throw new Error(
        session.connection.historyAvailable
          ? `Session ${id} is archived and no longer reconnectable`
          : `Session ${id} is not reconnectable`,
      )
    }

    await emitDiagnostic(id, "session_connected")
    return getSession(id)
  }

  async function getHistory(id: string): Promise<GetDaemonSessionHistoryResponse> {
    await ready
    const active = activeSessions.get(id)
    const session = await getSession(id)
    return {
      id: session.id,
      acpId: session.acpId,
      connection: session.connection,
      history: active ? [...active.history] : ((await SessionStateStorage.get(id))?.history ?? []),
    }
  }

  async function getDiagnostics(id: string): Promise<GetDaemonSessionDiagnosticsResponse> {
    await ready
    const session = await getSession(id)
    const state = await SessionStateStorage.get(id)
    return {
      id: session.id,
      acpId: session.acpId,
      connection: session.connection,
      events: (state?.diagnostics ?? []) as DaemonDiagnosticEvent[],
    }
  }

  async function sendMessage(id: string, message: acp.AnyMessage): Promise<void> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new Error(`Session ${id} is not active`)
    }

    if (
      active.lastPermissionRequest &&
      "id" in message &&
      message.id === active.lastPermissionRequest.id
    ) {
      active.lastPermissionRequest = null
    } else {
      const nextStatus = sessionStatusFromClientMessage(message, active.status)
      if (nextStatus) {
        await updateSession(active.id, { status: nextStatus })
      }

      if (
        active.isFirstPrompt &&
        isAcpRequest<PromptRequestMessage>(message, acp.AGENT_METHODS.session_prompt)
      ) {
        active.isFirstPrompt = false
        message.params = injectSystemPrompt(message.params, active.systemPrompt)
      }
    }

    if ("id" in message && message.id != null && "method" in message) {
      active.clientRequests.set(message.id, message as acp.AnyMessage & { method: string })
    }

    await appendHistory(active.id, message)
    await emitDiagnostic(active.id, "session_message_sent", {
      hasId: "id" in message && message.id != null,
      method: "method" in message ? message.method : undefined,
    })
    input.publish(active.id, message)
    await active.writer.write(message)
  }

  async function shutdownSession(id: string): Promise<boolean> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    await emitDiagnostic(id, "session_shutdown_requested")
    active.process.kill()
    return true
  }

  async function resolveSessionIdByToken(token: string): Promise<string> {
    await ready
    const record = await SessionPermissionsStorage.getByToken(token)
    if (!record) {
      throw new Error("Invalid session token")
    }

    return record.sessionId
  }

  async function close(): Promise<void> {
    await ready
    for (const session of activeSessions.values()) {
      await emitDiagnostic(session.id, "daemon_shutdown", { status: session.status })
      session.process.kill()
      await session.writer.close().catch(() => {})
      await session.subscription.close().catch(() => {})
      await SessionPermissionsStorage.revoke(session.id).catch(() => {})
    }
    activeSessions.clear()
  }
  return {
    createSession,
    connectSession,
    getSession,
    getHistory,
    getDiagnostics,
    sendMessage,
    shutdownSession,
    resolveSessionIdByToken,
    close,
  }
}
