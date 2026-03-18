import * as acp from "@agentclientprotocol/sdk"
import { ACPAdapterName } from "@goddard-ai/schema/acp-adapters"
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
import {
  agentBinaryPlatforms,
  type AgentBinaryPlatform,
  type AgentBinaryTarget,
  type AgentDistribution,
} from "@goddard-ai/schema/session-server"
import {
  SessionStateStorage,
  SessionStorage,
  type SessionConnectionMode,
  type SessionDiagnosticEvent,
  type SQLSessionUpdate,
} from "@goddard-ai/storage"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import treeKill from "@goddard-ai/tree-kill"
import { spawn, type ChildProcessByStdio } from "node:child_process"
import { randomBytes, randomUUID } from "node:crypto"
import { Readable, Writable } from "node:stream"
import { prependAgentBinToPath } from "../config.js"
import { createChunkPreview, createDaemonLogger, createPayloadPreview } from "../logging.js"
import {
  createAgentConnection,
  createAgentMessageStream,
  getAcpMessageResult,
  isAcpRequest,
  matchAcpRequest,
} from "./acp.js"
import { fetchRegistryAgent } from "./registry.js"

/** The current version of `@goddard-ai/daemon` */
declare const __VERSION__: string

/** Falls back to a safe placeholder when the build-time version constant is unavailable. */
function getPackageVersion(): string {
  try {
    return __VERSION__
  } catch {
    return "0.0.0"
  }
}

const logger = createDaemonLogger()

/** Tracks in-flight client requests so agent responses can be correlated back to session state. */
type ClientRequestMap = Map<string | number, acp.AnyMessage & { method: string }>

/** Represents the most recent permission request awaiting a client decision. */
type PermissionRequest = acp.AnyMessage & {
  id: unknown
  params: acp.RequestPermissionRequest
}

/** Captures prompt requests so their responses can drive status transitions. */
type PromptRequestMessage = acp.AnyMessage & {
  params: acp.PromptRequest
}

/** Holds the live runtime state for a daemon-owned session process. */
/** Pending daemon-owned prompt request waiting for the agent response frame. */
type PendingPromptRequest = {
  resolve: (response: acp.PromptResponse) => void
  reject: (error: Error) => void
}
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
  pendingPrompts: Map<string | number, PendingPromptRequest>
}

/** Exposes the daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = {
  createSession: (params: CreateDaemonSessionRequest) => Promise<DaemonSession>
  connectSession: (id: string) => Promise<DaemonSession>
  getSession: (id: string) => Promise<DaemonSession>
  getHistory: (id: string) => Promise<GetDaemonSessionHistoryResponse>
  getDiagnostics: (id: string) => Promise<GetDaemonSessionDiagnosticsResponse>
  sendMessage: (id: string, message: acp.AnyMessage) => Promise<void>
  promptSession: (id: string, prompt: string | acp.ContentBlock[]) => Promise<acp.PromptResponse>
  shutdownSession: (id: string) => Promise<boolean>
  resolveSessionIdByToken: (token: string) => Promise<string>
  close: () => Promise<void>
}

/** Ensures the daemon's system prompt is prepended to the first user prompt sent to an agent. */
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

/** Maps client-originated ACP messages to any immediate session status changes they imply. */
function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: SessionStatus,
): SessionStatus | null {
  if (status !== "active") {
    return null
  }

  if (isAcpRequest(message, acp.AGENT_METHODS.session_cancel)) {
    return "cancelled"
  }

  return null
}

/** Interprets agent responses in the context of the triggering client request. */
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

  if (getAcpMessageResult<acp.PromptResponse>(message)?.stopReason === "end_turn") {
    return "done"
  }

  return null
}

/** Treats abrupt termination signals as session errors instead of normal shutdowns. */
function isErrorSignal(signal: string | null): boolean {
  return signal === "SIGKILL" || signal === "SIGABRT" || signal === "SIGQUIT"
}

/** Detects one-shot sessions that should exit immediately after the initial prompt completes. */
function shouldExitAfterInitialPrompt(params: CreateDaemonSessionRequest): boolean {
  return params.oneShot === true && params.initialPrompt !== undefined
}

/** Normalizes persisted timestamps to ISO strings for schema responses. */
function toIsoString(value: Date | number): string {
  return (value instanceof Date ? value : new Date(value)).toISOString()
}

/** Derives reconnectability and history availability from stored connection state. */
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

/** Chooses the archived connection mode from whether any session history survived persistence. */
function archivedConnectionMode(historyLength: number): SessionConnectionMode {
  return historyLength > 0 ? "history" : "none"
}

/** Hydrates a stored session record with derived state needed by daemon clients. */
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

/** Produces a stable agent name whether the request used an id or a resolved distribution. */
function agentNameFromInput(agent: string | AgentDistribution): string {
  if (typeof agent === "string") {
    return agent
  }

  return agent.name
}

/** Builds the child-process environment expected by session agents. */
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

/** Resolves and launches the requested agent distribution for a new daemon session. */
async function spawnAgentProcess(
  daemonUrl: string,
  token: string,
  params: {
    agent: ACPAdapterName | AgentDistribution
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
    agent = fetchedAgent
  }

  const processSpec = resolveAgentProcessSpec(agent)

  return spawn(processSpec.cmd, processSpec.args, {
    stdio: ["pipe", "pipe", "inherit"],
    cwd: params.cwd,
    env: buildAgentProcessEnv({
      daemonUrl,
      token,
      agentBinDir: params.agentBinDir,
      env: {
        ...processSpec.env,
        ...params.env,
      },
    }),
  })
}

/** Chooses the concrete command invocation for a resolved agent distribution. */
function resolveAgentProcessSpec(agent: AgentDistribution): {
  cmd: string
  args: string[]
  env?: Record<string, string>
} {
  const binaryTarget = resolveBinaryTarget(agent)
  if (binaryTarget) {
    return {
      cmd: binaryTarget.cmd,
      args: binaryTarget.args ?? [],
      env: binaryTarget.env,
    }
  }

  if (agent.distribution.npx) {
    return {
      cmd: "npx",
      args: ["-y", agent.distribution.npx.package, ...(agent.distribution.npx.args ?? [])],
      env: agent.distribution.npx.env,
    }
  }

  if (agent.distribution.uvx) {
    return {
      cmd: "uvx",
      args: [agent.distribution.uvx.package, ...(agent.distribution.uvx.args ?? [])],
      env: agent.distribution.uvx.env,
    }
  }

  throw new Error(`Unsupported agent distribution for ${agent.id}`)
}

/** Selects the platform-specific binary target for the current runtime when available. */
function resolveBinaryTarget(agent: AgentDistribution): AgentBinaryTarget | null {
  const platformKey = toAgentBinaryPlatform(process.platform, process.arch)
  if (!platformKey) {
    return null
  }

  return agent.distribution.binary?.[platformKey] ?? null
}

/** Converts Node platform metadata into the registry's binary target keys. */
function toAgentBinaryPlatform(
  platform: NodeJS.Platform,
  arch: string,
): AgentBinaryPlatform | null {
  const normalizedPlatform =
    platform === "win32"
      ? "windows"
      : platform === "darwin"
        ? "darwin"
        : platform === "linux"
          ? "linux"
          : null
  const normalizedArch = arch === "arm64" ? "aarch64" : arch === "x64" ? "x86_64" : null
  if (!normalizedPlatform || !normalizedArch) {
    return null
  }

  const key = `${normalizedPlatform}-${normalizedArch}`
  return agentBinaryPlatforms.includes(key as AgentBinaryPlatform)
    ? (key as AgentBinaryPlatform)
    : null
}

/** Performs the ACP handshake and optional initial prompt before live streaming begins. */
async function initializeSession(
  input: Writable,
  output: Readable,
  params: CreateDaemonSessionRequest,
  hooks: {
    onMessageWrite?: (message: acp.AnyMessage) => void
  } = {},
): Promise<{
  status: SessionStatus
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  acpId: string
}> {
  const history: acp.AnyMessage[] = []
  const stream = createAgentMessageStream(input, output)

  const agent = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params: any) {
        const message = {
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params,
        } satisfies acp.AnyMessage
        history.push(message)
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
      hooks.onMessageWrite?.(history.at(-1)!)

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

/** Extracts repository ownership metadata used for permission scoping. */
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

/** Builds the structured logging context shared across session lifecycle events. */
function buildSessionLogContext(params: {
  agent: string | AgentDistribution
  cwd: string
  oneShot?: boolean
  metadata?: DaemonSessionMetadata
}): Record<string, unknown> {
  const metadata = params.metadata
  const workforceMetadata =
    metadata && typeof metadata === "object" && "workforce" in metadata
      ? (metadata.workforce as {
          rootDir?: unknown
          agentId?: unknown
          requestId?: unknown
        } | null)
      : null

  return {
    agent: agentNameFromInput(params.agent),
    cwd: params.cwd,
    oneShot: params.oneShot === true,
    repository: typeof metadata?.repository === "string" ? metadata.repository : undefined,
    prNumber: typeof metadata?.prNumber === "number" ? metadata.prNumber : undefined,
    workforceRootDir:
      workforceMetadata && typeof workforceMetadata.rootDir === "string"
        ? workforceMetadata.rootDir
        : undefined,
    workforceAgentId:
      workforceMetadata && typeof workforceMetadata.agentId === "string"
        ? workforceMetadata.agentId
        : undefined,
    workforceRequestId:
      workforceMetadata && typeof workforceMetadata.requestId === "string"
        ? workforceMetadata.requestId
        : undefined,
  }
}

/** Creates a normalized diagnostic record for persistence and log correlation. */
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

/** Logs raw transport chunks with a compact preview for debugging broken streams. */
function logAgentChunk(sessionId: string, acpId: string | undefined, chunk: Uint8Array): void {
  if (chunk.byteLength === 0) {
    return
  }

  logger.log("agent.chunk_read", {
    sessionId,
    acpId,
    preview: createChunkPreview(chunk),
  })
}

/** Logs ACP messages in a structured form without dumping full payloads verbatim. */
function logAgentMessage(
  event: "agent.message_read" | "agent.message_write",
  sessionId: string,
  acpId: string | undefined,
  message: acp.AnyMessage,
): void {
  logger.log(event, {
    sessionId,
    acpId,
    direction: event === "agent.message_read" ? "read" : "write",
    hasId: "id" in message && message.id != null,
    method: "method" in message ? message.method : undefined,
    message: createPayloadPreview(message),
  })
}

/** Creates the daemon session manager and owns reconciliation of live session processes. */
/** Resolves or rejects one pending prompt when its agent response frame arrives. */
function settlePendingPrompt(active: ActiveSession, message: acp.AnyMessage): void {
  if ("id" in message === false || message.id == null) {
    return
  }

  const pending = active.pendingPrompts.get(message.id)
  if (!pending) {
    return
  }

  active.pendingPrompts.delete(message.id)
  if ("error" in message) {
    pending.reject(new Error(resolveJsonRpcErrorMessage(message.error)))
    return
  }

  pending.resolve(message.result as acp.PromptResponse)
}

/** Rejects any in-flight prompt waits when a daemon session is torn down. */
function rejectPendingPrompts(active: ActiveSession, error: Error): void {
  for (const pending of active.pendingPrompts.values()) {
    pending.reject(error)
  }
  active.pendingPrompts.clear()
}

/** Formats one JSON-RPC error payload into a stable daemon error message. */
function resolveJsonRpcErrorMessage(error: {
  code?: number
  message?: string
  data?: unknown
}): string {
  if (typeof error.message === "string" && error.message.length > 0) {
    return error.message
  }

  if (typeof error.code === "number") {
    return `Agent request failed with code ${error.code}`
  }

  if (error.data !== undefined) {
    return `Agent request failed: ${JSON.stringify(error.data)}`
  }

  return "Agent request failed"
}
export function createSessionManager(input: {
  daemonUrl: string
  agentBinDir: string
  publish: (id: string, message: acp.AnyMessage) => void
}): SessionManager {
  const activeSessions = new Map<string, ActiveSession>()
  const ready = reconcilePersistedSessions()

  async function updateSession(
    id: string,
    update: SQLSessionUpdate,
    detail?: Record<string, unknown>,
  ): Promise<void> {
    const active = activeSessions.get(id)
    const previousStatus = active?.status ?? (await SessionStorage.get(id))?.status
    if (update.status && active) {
      active.status = update.status
    }

    await SessionStorage.update(id, update)
    if (update.status && previousStatus && previousStatus !== update.status) {
      await emitDiagnostic(id, "session_status_changed", {
        previousStatus,
        nextStatus: update.status,
        ...detail,
      })
    }
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
    logger.log(type, { sessionId, ...detail })
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
      logger.log("session_reconciliation_failed", {
        errorMessage: error instanceof Error ? error.message : String(error),
      })
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

        await setConnectionMode(
          session.id,
          archivedConnectionMode(state?.history.length ?? 0),
          false,
        )
        await SessionPermissionsStorage.revoke(session.id).catch(() => {})
      }),
    )
  }

  async function createSession(params: CreateDaemonSessionRequest): Promise<DaemonSession> {
    await ready
    const id = randomUUID()
    const token = randomBytes(32).toString("hex")
    const scope = parseRepoScope(params.metadata)
    const sessionLogContext = buildSessionLogContext(params)
    const sessionContext = {
      sessionId: id,
      acpId: undefined as string | undefined,
    }

    await SessionPermissionsStorage.create({
      sessionId: id,
      token,
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    })

    try {
      logger.log("session.launch_requested", {
        sessionId: id,
        ...sessionLogContext,
      })

      const process = await spawnAgentProcess(input.daemonUrl, token, {
        agent: params.agent,
        cwd: params.cwd,
        agentBinDir: input.agentBinDir,
        env: params.env,
      })

      const initialized = await initializeSession(process.stdin, process.stdout, params, {
        onMessageWrite: (message) => {
          logAgentMessage(
            "agent.message_write",
            sessionContext.sessionId,
            sessionContext.acpId,
            message,
          )
        },
      })
      sessionContext.acpId = initialized.acpId

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
        ...sessionLogContext,
      })

      if (shouldExitAfterInitialPrompt(params)) {
        // InitializeSession already sent the only prompt, so archive the history and tear the agent down.
        await updateSession(id, { status: "done" }, { reason: "one_shot_completed" })
        await setConnectionMode(id, "history", false)
        await emitDiagnostic(id, "session_completed_one_shot")
        await treeKill(process)
        await SessionPermissionsStorage.revoke(id).catch(() => {})
        return toDaemonSession(await SessionStorage.get(id))
      }

      const connection = createAgentConnection(process.stdin, process.stdout, {
        onChunk: (chunk) => {
          logAgentChunk(sessionContext.sessionId, sessionContext.acpId, chunk)
        },
        onMessageError: (error) => {
          logger.log("agent.message_handler_failed", {
            sessionId: sessionContext.sessionId,
            acpId: sessionContext.acpId,
            errorMessage: error instanceof Error ? error.message : String(error),
          })
        },
      })
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
        pendingPrompts: new Map(),
      }

      active.subscription = connection.subscribe(async (message) => {
        logAgentMessage("agent.message_read", active.id, active.acpId, message)
        if (
          isAcpRequest<PermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)
        ) {
          active.lastPermissionRequest = message
        } else if ("id" in message && message.id != null) {
          const clientRequest = active.clientRequests.get(message.id)
          const nextStatus = sessionStatusFromAgentMessage(clientRequest, message)
          if (nextStatus) {
            await updateSession(
              active.id,
              { status: nextStatus },
              {
                reason: "agent_message",
                requestMethod: clientRequest?.method,
                responseId: message.id,
              },
            )
          }
          if (clientRequest) {
            active.clientRequests.delete(message.id)
          }
          settlePendingPrompt(active, message)
        }

        await appendHistory(active.id, message)
        input.publish(active.id, message)
      })

      const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
        activeSessions.delete(active.id)
        rejectPendingPrompts(
          active,
          new Error(`Session ${active.id} ended before the prompt completed.`),
        )
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

        await setConnectionMode(active.id, archivedConnectionMode(active.history.length), false)
        await emitDiagnostic(active.id, "agent_process_exit", {
          code,
          signal,
          nextStatus: nextUpdate.status ?? active.status,
        })
        if (Object.keys(nextUpdate).length > 0) {
          await updateSession(active.id, nextUpdate, {
            reason: "agent_process_exit",
            code,
            signal,
          }).catch(() => {})
        }
      }

      process.once("exit", (code, signal) => {
        void handleExit(code, signal)
      })

      activeSessions.set(active.id, active)
      return toDaemonSession(await SessionStorage.get(id))
    } catch (error) {
      logger.log("session.launch_failed", {
        sessionId: id,
        ...sessionLogContext,
        errorMessage: error instanceof Error ? error.message : String(error),
      })
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
        await updateSession(
          active.id,
          { status: nextStatus },
          {
            reason: "client_message",
            method: "method" in message ? message.method : undefined,
            messageId: "id" in message ? message.id : undefined,
          },
        )
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

    logAgentMessage("agent.message_write", active.id, active.acpId, message)
    await appendHistory(active.id, message)
    await emitDiagnostic(active.id, "session_message_sent", {
      hasId: "id" in message && message.id != null,
      method: "method" in message ? message.method : undefined,
    })
    input.publish(active.id, message)
    await active.writer.write(message)
  }

  async function promptSession(
    id: string,
    prompt: string | acp.ContentBlock[],
  ): Promise<acp.PromptResponse> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new Error(`Session ${id} is not active`)
    }

    const requestId = randomUUID()
    const response = new Promise<acp.PromptResponse>((resolve, reject) => {
      active.pendingPrompts.set(requestId, {
        resolve,
        reject,
      })
    })

    try {
      await sendMessage(id, {
        jsonrpc: "2.0",
        id: requestId,
        method: acp.AGENT_METHODS.session_prompt,
        params: {
          sessionId: active.acpId,
          prompt: typeof prompt === "string" ? [{ type: "text", text: prompt }] : prompt,
        },
      } satisfies acp.AnyMessage)
      return await response
    } catch (error) {
      active.pendingPrompts.delete(requestId)
      throw error
    }
  }

  async function shutdownSession(id: string): Promise<boolean> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    await emitDiagnostic(id, "session_shutdown_requested")
    await treeKill(active.process)
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
      await treeKill(session.process)
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
    promptSession,
    shutdownSession,
    resolveSessionIdByToken,
    close,
  }
}
