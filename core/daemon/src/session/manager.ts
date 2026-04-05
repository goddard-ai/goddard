import * as acp from "@agentclientprotocol/sdk"
import { IpcClientError } from "@goddard-ai/ipc"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import type { ACPAdapterName } from "@goddard-ai/schema/acp-adapters"
import type { UserConfig } from "@goddard-ai/schema/config"
import type {
  CreateDaemonSessionRequest,
  DaemonDiagnosticEvent,
  DaemonSession,
  DaemonSessionConnection,
  DaemonSessionMetadata,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionWorkforceResponse,
  GetDaemonSessionWorktreeResponse,
  ListDaemonSessionsRequest,
  ListDaemonSessionsResponse,
} from "@goddard-ai/schema/daemon"
import { InitialPromptOption } from "@goddard-ai/schema/daemon/sessions"
import type { SessionStatus } from "@goddard-ai/schema/db"
import {
  agentBinaryPlatforms,
  type AgentBinaryPlatform,
  type AgentBinaryTarget,
  type AgentDistribution,
} from "@goddard-ai/schema/session-server"
import treeKill, { type ProcessLike } from "@goddard-ai/tree-kill"
import type { WorktreePlugin } from "@goddard-ai/worktree"
import type { KindInput, KindOutput } from "kindstore"
import { createHash, randomBytes, randomUUID } from "node:crypto"
import { constants as fsConstants } from "node:fs"
import { access, mkdir, mkdtemp, readdir, rename, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { Readable, Writable } from "node:stream"
import { ReadableStream } from "node:stream/web"
import { prependAgentBinToPath } from "../config.ts"
import { createChunkPreview, createDaemonLogger, createPayloadPreview } from "../logging.ts"
import {
  type SessionConnectionMode,
  type SessionDiagnosticEvent,
} from "../persistence/session-state.ts"
import { db } from "../persistence/store.ts"
import {
  createAgentConnection,
  createAgentMessageStream,
  getAcpMessageResult,
  isAcpRequest,
  matchAcpRequest,
  type AgentInputStream,
  type AgentOutputStream,
} from "./acp.ts"
import {
  binaryInstallMarkerFileName,
  installBinaryTargetPayload,
  resolveInstalledBinaryCommand,
} from "./archive.ts"
import { fetchRegistryAgent } from "./registry.ts"
import { prepareSessionWorktree } from "./worktree.ts"

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
type DaemonSessionId = DaemonSession["id"]

/** Returns true when one filesystem path currently exists. */
async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Tracks in-flight client requests so agent responses can be correlated back to session state. */
type ClientRequestMap = Map<string | number, acp.AnyMessage & { method: string }>

/** Describes the concrete child-process invocation for a resolved agent distribution. */
type AgentProcessSpec = {
  cmd: string
  args: string[]
  env?: Record<string, string>
}

/** Represents the most recent permission request awaiting a client decision. */
type PermissionRequest = acp.AnyMessage & {
  id: unknown
  params: acp.RequestPermissionRequest
}

/** Captures prompt requests so their responses can drive status transitions. */
type PromptRequestMessage = acp.AnyMessage & {
  params: acp.PromptRequest
}

/** Pending daemon-owned prompt request waiting for the agent response frame. */
type PendingPromptRequest = {
  resolve: (response: acp.PromptResponse) => void
  reject: (error: Error) => void
}

/** Couples a binary target with the platform key that selected it. */
type ResolvedBinaryTarget = {
  platformKey: AgentBinaryPlatform
  target: AgentBinaryTarget
}

/** Callback fired when one Bun-managed agent process exits. */
type AgentProcessExitHandler = (code: number | null, signal: NodeJS.Signals | null) => void

/** Bun subprocess wrapper that preserves the exit hooks and stdio surface the daemon expects. */
type AgentProcessHandle = ProcessLike & {
  stdin: AgentInputStream
  stdout: AgentOutputStream
  onceExit: (handler: AgentProcessExitHandler) => void
}

/** Holds the live runtime state for a daemon-owned session process. */
type ActiveSession = {
  id: DaemonSessionId
  acpSessionId: string
  token: string
  process: AgentProcessHandle
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

/** Shared session-launch options resolved by the daemon before an agent process starts. */
interface SessionLaunchParams {
  request: CreateDaemonSessionRequest
  token?: string
  config?: UserConfig
  worktreePlugins?: WorktreePlugin[]
}

/** Fresh daemon session input accepted by `SessionManager.newSession()`. */
interface NewSessionParams extends SessionLaunchParams {}

/** Stored daemon session input accepted by `SessionManager.loadSession()`. */
interface LoadSessionParams extends SessionLaunchParams {
  id: DaemonSessionId
}

/** Exposes the daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = {
  newSession: (params: NewSessionParams) => Promise<DaemonSession>
  loadSession: (params: LoadSessionParams) => Promise<DaemonSession>
  listSessions: (params: ListDaemonSessionsRequest) => Promise<ListDaemonSessionsResponse>
  connectSession: (id: DaemonSessionId) => Promise<DaemonSession>
  getSession: (id: DaemonSessionId) => Promise<DaemonSession>
  getHistory: (id: DaemonSessionId) => Promise<GetDaemonSessionHistoryResponse>
  getDiagnostics: (id: DaemonSessionId) => Promise<GetDaemonSessionDiagnosticsResponse>
  getWorktree: (id: DaemonSessionId) => Promise<GetDaemonSessionWorktreeResponse>
  getWorkforce: (id: DaemonSessionId) => Promise<GetDaemonSessionWorkforceResponse>
  sendMessage: (id: DaemonSessionId, message: acp.AnyMessage) => Promise<void>
  promptSession: (
    id: DaemonSessionId,
    prompt: string | acp.ContentBlock[],
  ) => Promise<acp.PromptResponse>
  shutdownSession: (id: DaemonSessionId) => Promise<boolean>
  resolveSessionIdByToken: (token: string) => Promise<DaemonSessionId>
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

function createInitialPromptRequest(params: {
  sessionId: string
  prompt: InitialPromptOption
  isFirstPrompt: boolean
  systemPrompt: string
}) {
  const promptRequest = {
    sessionId: params.sessionId,
    prompt:
      typeof params.prompt === "string" ? [{ type: "text", text: params.prompt }] : params.prompt,
  } satisfies acp.PromptRequest

  return params.isFirstPrompt
    ? injectSystemPrompt(promptRequest, params.systemPrompt)
    : promptRequest
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
function shouldExitAfterInitialPrompt(params: SessionLaunchParams): boolean {
  return params.request.oneShot === true && params.request.initialPrompt !== undefined
}

/** Derives reconnectability from stored connection state without joining adjacent kinds. */
function toConnectionState(input: {
  mode: SessionConnectionMode
  activeDaemonSession: boolean
}): DaemonSessionConnection {
  return {
    mode: input.mode,
    reconnectable: input.mode === "live" && input.activeDaemonSession,
    activeDaemonSession: input.activeDaemonSession,
  }
}

/** Chooses the archived connection mode from whether any session history survived persistence. */
function archivedConnectionMode(historyLength: number): SessionConnectionMode {
  return historyLength > 0 ? "history" : "none"
}

/** Merges structured session metadata layers while dropping empty results. */
function mergeSessionMetadata(
  a: DaemonSessionMetadata | null | undefined,
  b: DaemonSessionMetadata | null | undefined,
): DaemonSessionMetadata | undefined {
  const merged = { ...a, ...b }
  return Object.keys(merged).length > 0 ? merged : undefined
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

/** Wraps Bun's subprocess API with the minimal process hooks used by session management. */
function createAgentProcessHandle(input: {
  cmd: string
  args: string[]
  cwd: string
  env: NodeJS.ProcessEnv
}): AgentProcessHandle {
  let exitState: { code: number | null; signal: NodeJS.Signals | null } | null = null
  const exitHandlers = new Set<AgentProcessExitHandler>()
  const subprocess = Bun.spawn([input.cmd, ...input.args], {
    cwd: input.cwd,
    env: input.env,
    stdin: "pipe",
    stdout: "pipe",
    stderr: "inherit",
    onExit(_subprocess, exitCode, signalCode) {
      exitState = {
        code: exitCode,
        signal: signalCode as NodeJS.Signals | null,
      }

      for (const handler of exitHandlers) {
        handler(exitState.code, exitState.signal)
      }
      exitHandlers.clear()
    },
  })

  if (!subprocess.stdin || !subprocess.stdout) {
    throw new Error(`Agent process ${input.cmd} did not expose piped stdio`)
  }

  const stdin = new Writable({
    write(chunk, _encoding, callback) {
      Promise.resolve(subprocess.stdin!.write(chunk))
        .then(() => callback())
        .catch((error) => {
          callback(error instanceof Error ? error : new Error(String(error)))
        })
    },
    final(callback) {
      Promise.resolve(subprocess.stdin!.end())
        .then(() => callback())
        .catch((error) => {
          callback(error instanceof Error ? error : new Error(String(error)))
        })
    },
  })
  const stdout = Readable.fromWeb(subprocess.stdout as unknown as ReadableStream)

  return {
    stdin,
    stdout,
    pid: subprocess.pid,
    kill(signal) {
      subprocess.kill(signal as never)
      return true
    },
    onceExit(handler) {
      if (exitState) {
        handler(exitState.code, exitState.signal)
        return
      }

      const wrapped: AgentProcessExitHandler = (code, signal) => {
        exitHandlers.delete(wrapped)
        handler(code, signal)
      }
      exitHandlers.add(wrapped)
    },
  }
}

/** Resolves and launches the requested agent distribution for a new daemon session. */
export async function spawnAgentProcess(params: {
  daemonUrl: string
  token: string
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  agentBinDir: string
  env?: Record<string, string>
  registry?: Record<string, AgentDistribution>
}): Promise<AgentProcessHandle> {
  let agent = params.agent

  if (typeof agent === "string") {
    if (params.registry?.[agent]) {
      agent = params.registry[agent]
    } else {
      const fetchedAgent = await fetchRegistryAgent(agent)
      if (!fetchedAgent) {
        throw new Error(`Agent not found: ${agent}`)
      }
      agent = fetchedAgent
    }
  }

  const { cmd, args, env } = await resolveAgentProcessSpec(agent)

  return createAgentProcessHandle({
    cmd,
    args,
    cwd: params.cwd,
    env: buildAgentProcessEnv({
      daemonUrl: params.daemonUrl,
      token: params.token,
      agentBinDir: params.agentBinDir,
      env: {
        ...env,
        ...params.env,
      },
    }),
  })
}

/** Chooses the concrete command invocation for a resolved agent distribution. */
export async function resolveAgentProcessSpec(agent: AgentDistribution): Promise<AgentProcessSpec> {
  const binaryTarget = resolveBinaryTarget(agent)
  if (binaryTarget) {
    return {
      cmd: await resolveBinaryCommand(agent, binaryTarget),
      args: binaryTarget.target.args ?? [],
      env: binaryTarget.target.env,
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
function resolveBinaryTarget(agent: AgentDistribution): ResolvedBinaryTarget | null {
  const platformKey = toAgentBinaryPlatform(process.platform, process.arch)
  if (!platformKey) {
    return null
  }

  const target = agent.distribution.binary?.[platformKey]
  if (!target) {
    return null
  }

  return {
    platformKey,
    target,
  }
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

/** Resolves the runnable binary path for an archive-backed target, installing it into the global cache first. */
async function resolveBinaryCommand(
  agent: AgentDistribution,
  binaryTarget: ResolvedBinaryTarget,
): Promise<string> {
  const installDir = getBinaryInstallDir(agent, binaryTarget)
  const installMarkerPath = join(installDir, binaryInstallMarkerFileName)

  if (!(await pathExists(installMarkerPath))) {
    await installBinaryArchive(
      agent.id,
      binaryTarget.target.archive,
      binaryTarget.target.cmd,
      installDir,
    )
  }

  await cleanupOtherAgentBinaryInstalls(agent.id, installDir)

  return await resolveInstalledBinaryCommand(installDir, binaryTarget.target.cmd)
}

/** Computes the global cache directory for one archive-backed binary target. */
function getBinaryInstallDir(agent: AgentDistribution, binaryTarget: ResolvedBinaryTarget): string {
  const archiveHash = createHash("sha256")
    .update(binaryTarget.target.archive)
    .digest("hex")
    .slice(0, 12)

  return join(
    getGoddardGlobalDir(),
    "binaries",
    `${agent.id}-${agent.version}-${binaryTarget.platformKey}-${archiveHash}`,
  )
}

/** Downloads and installs one archive-backed or raw binary target into its final global cache directory. */
async function installBinaryArchive(
  agentId: string,
  archiveUrl: string,
  cmd: string,
  installDir: string,
): Promise<void> {
  const binariesDir = join(getGoddardGlobalDir(), "binaries")
  await mkdir(binariesDir, { recursive: true })
  await rm(installDir, { recursive: true, force: true })

  const stagingParentDir = await mkdtemp(join(binariesDir, "install-"))
  const stagedInstallDir = join(stagingParentDir, "install")

  try {
    await installBinaryTargetPayload({
      archiveUrl,
      cmd,
      installDir: stagedInstallDir,
    })
    await writeFile(join(stagedInstallDir, binaryInstallMarkerFileName), `${archiveUrl}\n`, "utf8")
    await rename(stagedInstallDir, installDir)
    await cleanupOtherAgentBinaryInstalls(agentId, installDir)
  } finally {
    await rm(stagingParentDir, { recursive: true, force: true })
  }
}

/** Removes cached binary installs for the same agent id except for the active install directory. */
async function cleanupOtherAgentBinaryInstalls(
  agentId: string,
  activeInstallDir: string,
): Promise<void> {
  const binariesDir = join(getGoddardGlobalDir(), "binaries")
  if (!(await pathExists(binariesDir))) {
    return
  }

  const agentPrefix = `${agentId}-`
  const installs = await readdir(binariesDir, { withFileTypes: true })

  await Promise.all(
    installs
      .filter(
        (entry) =>
          entry.isDirectory() &&
          entry.name.startsWith(agentPrefix) &&
          join(binariesDir, entry.name) !== activeInstallDir,
      )
      .map((entry) => rm(join(binariesDir, entry.name), { recursive: true, force: true })),
  )
}

/** Performs the ACP handshake and optional initial prompt before live streaming begins. */
async function initializeSession(params: {
  input: AgentInputStream
  output: AgentOutputStream
  request: CreateDaemonSessionRequest
  resumeAcpId?: string
  onMessageWrite?: (message: acp.AnyMessage) => void
}): Promise<
  acp.InitializeResponse & {
    status: SessionStatus
    isFirstPrompt: boolean
    history: acp.AnyMessage[]
    acpSessionId: string
    models?: acp.SessionModelState | null
  }
> {
  const history: acp.AnyMessage[] = []
  const stream = createAgentMessageStream(params.input, params.output)

  try {
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

    const initializeResult = await agent.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: { name: "npm:@goddard-ai/daemon", version: getPackageVersion() },
    })

    let status: SessionStatus = "active"
    let isFirstPrompt = true
    let acpSessionId: string
    let models: acp.SessionModelState | null | undefined

    if (
      params.resumeAcpId !== undefined &&
      initializeResult.agentCapabilities?.loadSession === true
    ) {
      await agent.loadSession({
        sessionId: params.resumeAcpId,
        cwd: params.request.cwd,
        mcpServers: params.request.mcpServers,
      })
      acpSessionId = params.resumeAcpId
      isFirstPrompt = false
    } else {
      const newSession = await agent.newSession(params.request)
      acpSessionId = newSession.sessionId
      models = newSession.models
    }

    if (params.request.initialPrompt !== undefined) {
      const initialMessage = {
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_prompt,
        params: createInitialPromptRequest({
          sessionId: acpSessionId,
          prompt: params.request.initialPrompt,
          isFirstPrompt,
          systemPrompt: params.request.systemPrompt,
        }),
      } satisfies acp.AnyMessage

      history.push(initialMessage)
      params.onMessageWrite?.(initialMessage)

      const response = await agent.prompt(initialMessage.params)
      switch (response.stopReason) {
        case "cancelled":
          status = "cancelled"
          break
        case "end_turn":
        case "max_tokens":
        case "max_turn_requests":
        case "refusal":
          status = "done"
          break
        default:
          response.stopReason satisfies never
      }
      isFirstPrompt = false
    }

    return {
      ...initializeResult,
      status,
      isFirstPrompt,
      history,
      acpSessionId,
      models,
    }
  } finally {
    await stream.readable.cancel().catch(() => {})
    await stream.writable.close().catch(() => {})
  }
}

/** Extracts repository ownership fields used for permission scoping and persistence. */
function parseRepoScope(params: { repository?: string; prNumber?: number }): {
  repository: string | null
  prNumber: number | null
  owner: string
  repo: string
  allowedPrNumbers: number[]
} {
  const repository = params.repository?.trim() ?? ""
  const prNumber = typeof params.prNumber === "number" ? params.prNumber : null
  const [owner, repo] = repository.split("/")

  return {
    repository: repository.length > 0 ? repository : null,
    prNumber,
    owner: owner ?? "",
    repo: repo ?? "",
    allowedPrNumbers: prNumber === null ? [] : [prNumber],
  }
}

/** Builds the structured logging context shared across session lifecycle events. */
function buildSessionLogContext(params: {
  request: CreateDaemonSessionRequest
  cwd?: string
  workforce?: CreateDaemonSessionRequest["workforce"]
  extraContext?: Record<string, unknown>
}): Record<string, unknown> {
  return {
    agent: agentNameFromInput(params.request.agent),
    cwd: params.cwd ?? params.request.cwd,
    oneShot: params.request.oneShot === true,
    repository:
      typeof params.request.repository === "string" ? params.request.repository : undefined,
    prNumber: typeof params.request.prNumber === "number" ? params.request.prNumber : undefined,
    workforce: params.workforce ?? params.request.workforce,
    ...params.extraContext,
  }
}

/** Creates a normalized diagnostic record for persistence and log correlation. */
function createDiagnosticEvent(
  type: string,
  detail?: Record<string, unknown>,
): SessionDiagnosticEvent {
  return {
    type,
    at: new Date().toISOString(),
    detail,
  }
}

/** Logs raw transport chunks with a compact preview for debugging broken streams. */
function logAgentChunk(
  sessionId: string,
  acpSessionId: string | undefined,
  chunk: Uint8Array,
): void {
  if (chunk.byteLength === 0) {
    return
  }

  logger.log("agent.chunk_read", {
    sessionId,
    acpSessionId,
    preview: createChunkPreview(chunk),
  })
}

/** Logs ACP messages in a structured form without dumping full payloads verbatim. */
function logAgentMessage(
  event: "agent.message_read" | "agent.message_write",
  sessionId: string,
  acpSessionId: string | undefined,
  message: acp.AnyMessage,
): void {
  logger.log(event, {
    sessionId,
    acpSessionId,
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
  } else if ("result" in message) {
    pending.resolve(getAcpMessageResult<acp.PromptResponse>(message))
  }
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

const DEFAULT_SESSION_PAGE_SIZE = 20
const MAX_SESSION_PAGE_SIZE = 100

/** Normalizes optional session page sizes to the daemon's supported bounds. */
function normalizeSessionPageSize(limit?: number): number {
  if (!Number.isFinite(limit)) {
    return DEFAULT_SESSION_PAGE_SIZE
  }

  return Math.min(
    Math.max(Math.trunc(limit ?? DEFAULT_SESSION_PAGE_SIZE), 1),
    MAX_SESSION_PAGE_SIZE,
  )
}

/** Creates the daemon-owned session lifecycle boundary over storage and agent processes. */
export function createSessionManager(input: {
  daemonUrl: string
  agentBinDir: string
  publish: (id: DaemonSessionId, message: acp.AnyMessage) => void
  registry?: Record<string, AgentDistribution>
}): SessionManager {
  const activeSessions = new Map<DaemonSessionId, ActiveSession>()
  const ready = reconcilePersistedSessions()

  async function updateSession(
    id: DaemonSessionId,
    update: Partial<KindInput<typeof db.schema.sessions>>,
    detail?: Record<string, unknown>,
  ): Promise<void> {
    const active = activeSessions.get(id)
    const previousRecord = db.sessions.get(id) ?? null
    const previousStatus = active?.status ?? previousRecord?.status
    if (update.status && active) {
      active.status = update.status
    }

    if (previousRecord) {
      db.sessions.update(previousRecord.id, update)
    }
    if (update.status && previousStatus && previousStatus !== update.status) {
      await emitDiagnostic(id, "session_status_changed", {
        previousStatus,
        nextStatus: update.status,
        ...detail,
      })
    }
  }

  async function appendHistory(id: DaemonSessionId, message: acp.AnyMessage): Promise<void> {
    const active = activeSessions.get(id)
    if (active) {
      active.history.push(message)
    }
    const historyRecord =
      db.sessionMessages.first({
        where: { sessionId: id },
      }) ?? null
    if (historyRecord) {
      db.sessionMessages.update(historyRecord.id, {
        messages: [...historyRecord.messages, message],
      })
      return
    }

    db.sessionMessages.create({
      sessionId: id,
      messages: [message],
    })
  }

  async function emitDiagnostic(
    sessionId: DaemonSessionId,
    type: string,
    detail?: Record<string, unknown>,
  ): Promise<void> {
    const event = createDiagnosticEvent(type, detail)
    logger.log(type, { sessionId, ...detail })
    const diagnosticsRecord =
      db.sessionDiagnostics.first({
        where: { sessionId },
      }) ?? null
    if (diagnosticsRecord) {
      db.sessionDiagnostics.update(diagnosticsRecord.id, {
        events: [...diagnosticsRecord.events, event],
      })
      return
    }

    db.sessionDiagnostics.create({
      sessionId,
      events: [event],
    })
  }

  async function setConnectionMode(
    sessionId: DaemonSessionId,
    mode: SessionConnectionMode,
    activeDaemonSession: boolean,
  ): Promise<void> {
    const sessionRecord = db.sessions.get(sessionId) ?? null
    if (!sessionRecord) {
      return
    }

    db.sessions.update(sessionRecord.id, {
      connectionMode: mode,
      activeDaemonSession,
    })
  }

  async function reconcilePersistedSessions(): Promise<void> {
    let persistedSessions: KindOutput<typeof db.schema.sessions>[]

    try {
      persistedSessions = await Promise.resolve(db.sessions.findMany())
    } catch (error) {
      logger.log("session_reconciliation_failed", {
        errorMessage: error instanceof Error ? error.message : String(error),
      })
      return
    }

    await Promise.all(
      persistedSessions.map(async (session) => {
        const messagesRecord =
          db.sessionMessages.first({
            where: { sessionId: session.id },
          }) ?? null
        if (!messagesRecord) {
          db.sessionMessages.create({
            sessionId: session.id,
            messages: [],
          })
        }

        const diagnosticsRecord =
          db.sessionDiagnostics.first({
            where: { sessionId: session.id },
          }) ?? null
        if (!diagnosticsRecord) {
          db.sessionDiagnostics.create({
            sessionId: session.id,
            events: [],
          })
        }

        if (
          session.status === "active" ||
          session.status === "blocked" ||
          session.status === "idle"
        ) {
          const sessionDocument = db.sessions.get(session.id) ?? null
          if (sessionDocument) {
            db.sessions.update(session.id, {
              status: "error",
              errorMessage: "Session interrupted when the previous daemon exited unexpectedly.",
              token: null,
              permissions: null,
            })
          }
          await setConnectionMode(session.id, "history", false)
          await emitDiagnostic(session.id, "session_reconciled_after_restart", {
            previousStatus: session.status,
          })
          return
        }

        await setConnectionMode(
          session.id,
          archivedConnectionMode(messagesRecord?.messages.length ?? 0),
          false,
        )
        if (session.permissions) {
          const sessionRecord = db.sessions.get(session.id) ?? null
          if (sessionRecord) {
            db.sessions.update(session.id, {
              token: null,
              permissions: null,
            })
          }
        }
      }),
    )
  }

  async function launchSession(
    params: SessionLaunchParams,
    existingSession: KindOutput<typeof db.schema.sessions> | null = null,
  ): Promise<DaemonSession> {
    await ready
    const id = existingSession?.id ?? db.sessions.newId()
    const token = params.token ?? randomBytes(32).toString("hex")

    const existingMessagesRecord = existingSession
      ? db.sessionMessages.first({
          where: { sessionId: id },
        })
      : null

    const existingWorktree = existingSession
      ? db.worktrees.first({
          where: { sessionId: id },
        })
      : null

    const existingWorkforce = existingSession
      ? db.workforces.first({
          where: { sessionId: id },
        })
      : null

    const worktree =
      existingWorktree != null || params.request.worktree?.enabled === true
        ? await prepareSessionWorktree(id, params.request.cwd, {
            branchNameOverride:
              typeof params.request.prNumber === "number"
                ? `pr-${params.request.prNumber}`
                : undefined,
            worktreePlugins: params.worktreePlugins,
            existingFolder: existingWorktree?.worktreeDir,
            defaultWorktreesFolder: params.config?.worktrees?.defaultFolder,
          })
        : null

    const cwd = worktree?.effectiveCwd ?? params.request.cwd
    const sessionMetadata = mergeSessionMetadata(existingSession?.metadata, params.request.metadata)
    const existingWorkforceMetadata = existingWorkforce
      ? (({ id: _id, sessionId: _sessionId, ...value }) => value)(existingWorkforce)
      : undefined
    const workforceMetadata = params.request.workforce ?? existingWorkforceMetadata
    const sessionContext = {
      sessionId: id,
      acpSessionId: undefined as string | undefined,
    }

    const sessionLogContext = buildSessionLogContext({
      request: params.request,
      cwd,
      workforce: workforceMetadata ?? undefined,
      extraContext: worktree
        ? {
            worktreeDir: worktree.state.worktreeDir,
            worktreePoweredBy: worktree.state.poweredBy,
          }
        : undefined,
    })

    const scope = parseRepoScope(params.request)

    const nextPermission = {
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    }

    try {
      logger.log("session.launch_requested", {
        sessionId: id,
        ...sessionLogContext,
      })

      const agentProcess = await spawnAgentProcess({
        daemonUrl: input.daemonUrl,
        token,
        agent: params.request.agent,
        cwd,
        agentBinDir: input.agentBinDir,
        env: params.request.env,
        registry: input.registry,
      })

      const initialized = await initializeSession({
        input: agentProcess.stdin,
        output: agentProcess.stdout,
        request: {
          ...params.request,
          cwd,
          metadata: sessionMetadata,
        },
        resumeAcpId: existingSession?.acpSessionId,
        onMessageWrite: (message) => {
          logAgentMessage(
            "agent.message_write",
            sessionContext.sessionId,
            sessionContext.acpSessionId,
            message,
          )
        },
      })
      sessionContext.acpSessionId = initialized.acpSessionId

      const initialHistory = existingMessagesRecord
        ? initialized.acpSessionId === existingSession?.acpSessionId
          ? existingMessagesRecord.messages.length > 0
            ? [...existingMessagesRecord.messages]
            : [...initialized.history]
          : [...existingMessagesRecord.messages, ...initialized.history]
        : [...initialized.history]

      if (existingMessagesRecord) {
        db.sessionMessages.put(existingMessagesRecord.id, {
          sessionId: id,
          messages: initialHistory,
        })
      } else {
        db.sessionMessages.create({
          sessionId: id,
          messages: initialHistory,
        })
      }

      if (worktree) {
        const nextWorktree = {
          sessionId: id,
          ...worktree.state,
        }
        if (existingWorktree) {
          db.worktrees.put(existingWorktree.id, nextWorktree)
        } else {
          db.worktrees.create(nextWorktree)
        }
      }

      if (workforceMetadata) {
        const nextWorkforce = {
          sessionId: id,
          ...workforceMetadata,
        }
        if (existingWorkforce) {
          db.workforces.put(existingWorkforce.id, nextWorkforce)
        } else {
          db.workforces.create(nextWorkforce)
        }
      }

      if (existingSession) {
        const existingDocument = db.sessions.get(id) ?? null
        if (!existingDocument) {
          throw new IpcClientError(`Cannot update unknown session: ${id}`)
        }

        db.sessions.update(id, {
          acpSessionId: initialized.acpSessionId,
          status: initialized.status,
          agentName: agentNameFromInput(params.request.agent),
          cwd,
          mcpServers: params.request.mcpServers,
          connectionMode: shouldExitAfterInitialPrompt(params) ? "history" : "live",
          activeDaemonSession: !shouldExitAfterInitialPrompt(params),
          repository: scope.repository,
          prNumber: scope.prNumber,
          token,
          permissions: nextPermission,
          metadata: sessionMetadata ?? null,
          models: initialized.models ?? existingSession.models ?? null,
          errorMessage: null,
          blockedReason: null,
          initiative: null,
          lastAgentMessage: null,
        })
      } else {
        db.sessions.put(id, {
          acpSessionId: initialized.acpSessionId,
          status: initialized.status,
          agentName: agentNameFromInput(params.request.agent),
          cwd,
          mcpServers: params.request.mcpServers,
          connectionMode: shouldExitAfterInitialPrompt(params) ? "history" : "live",
          activeDaemonSession: !shouldExitAfterInitialPrompt(params),
          repository: scope.repository,
          prNumber: scope.prNumber,
          token,
          permissions: nextPermission,
          metadata: sessionMetadata ?? null,
          models: initialized.models ?? null,
          errorMessage: null,
          blockedReason: null,
          initiative: null,
          lastAgentMessage: null,
        })
      }
      await emitDiagnostic(id, "session_created", {
        status: initialized.status,
        ...sessionLogContext,
      })

      if (shouldExitAfterInitialPrompt(params)) {
        agentProcess.onceExit((code, signal) => {
          void emitDiagnostic(id, "agent_process_exit", {
            code,
            signal,
            nextStatus: "done",
          }).catch(console.error)
        })

        // InitializeSession already sent the only prompt, so archive the history and tear the agent down.
        await updateSession(
          id,
          { status: "done", token: null, permissions: null },
          { reason: "one_shot_completed" },
        )
        await setConnectionMode(id, "history", false)
        await emitDiagnostic(id, "session_completed_one_shot")
        await treeKill(agentProcess)
        const sessionDocument = db.sessions.get(id) ?? null
        if (!sessionDocument) {
          throw new IpcClientError("Session not found")
        }
        return sessionDocument
      }

      const connection = createAgentConnection(agentProcess.stdin, agentProcess.stdout, {
        onChunk: (chunk) => {
          logAgentChunk(sessionContext.sessionId, sessionContext.acpSessionId, chunk)
        },
        onMessageError: (error) => {
          logger.log("agent.message_handler_failed", {
            sessionId: sessionContext.sessionId,
            acpSessionId: sessionContext.acpSessionId,
            errorMessage: error instanceof Error ? error.message : String(error),
          })
        },
      })
      const writer = connection.getWriter()
      const activeSession: ActiveSession = {
        id,
        acpSessionId: initialized.acpSessionId,
        token,
        process: agentProcess,
        writer,
        subscription: { close: async () => {} },
        status: initialized.status,
        history: initialHistory,
        isFirstPrompt: initialized.isFirstPrompt,
        systemPrompt: params.request.systemPrompt,
        lastPermissionRequest: null,
        clientRequests: new Map(),
        pendingPrompts: new Map(),
      }

      activeSession.subscription = connection.subscribe(async (message) => {
        logAgentMessage("agent.message_read", activeSession.id, activeSession.acpSessionId, message)
        if (
          isAcpRequest<PermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)
        ) {
          activeSession.lastPermissionRequest = message
        } else if ("id" in message && message.id != null) {
          const clientRequest = activeSession.clientRequests.get(message.id)
          const nextStatus = sessionStatusFromAgentMessage(clientRequest, message)
          if (nextStatus) {
            await updateSession(
              activeSession.id,
              { status: nextStatus },
              {
                reason: "agent_message",
                requestMethod: clientRequest?.method,
                responseId: message.id,
              },
            )
          }
          if (clientRequest) {
            activeSession.clientRequests.delete(message.id)
          }
          settlePendingPrompt(activeSession, message)
        }

        await appendHistory(activeSession.id, message)
        input.publish(activeSession.id, message)
      })

      const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
        activeSessions.delete(activeSession.id)
        rejectPendingPrompts(
          activeSession,
          new Error(`Session ${activeSession.id} ended before the prompt completed.`),
        )
        await activeSession.writer.close().catch(() => {})
        await activeSession.subscription.close().catch(() => {})

        const nextUpdate: Partial<KindInput<typeof db.schema.sessions>> = {}
        if (code !== 0 && code !== null) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Exited with code ${code}`
        } else if (isErrorSignal(signal)) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Killed by ${signal}`
        } else if (activeSession.status !== "done") {
          nextUpdate.status = "cancelled"
        }
        nextUpdate.token = null
        nextUpdate.permissions = null

        await setConnectionMode(
          activeSession.id,
          archivedConnectionMode(activeSession.history.length),
          false,
        )
        await emitDiagnostic(activeSession.id, "agent_process_exit", {
          code,
          signal,
          nextStatus: nextUpdate.status ?? activeSession.status,
        })
        if (Object.keys(nextUpdate).length > 0) {
          await updateSession(activeSession.id, nextUpdate, {
            reason: "agent_process_exit",
            code,
            signal,
          }).catch(() => {})
        }
      }

      agentProcess.onceExit((code, signal) => {
        void handleExit(code, signal)
      })

      activeSessions.set(activeSession.id, activeSession)
      const sessionDocument = db.sessions.get(id) ?? null
      if (!sessionDocument) {
        throw new IpcClientError("Session not found")
      }
      return sessionDocument
    } catch (error) {
      logger.log("session.launch_failed", {
        sessionId: id,
        ...sessionLogContext,
        errorMessage: error instanceof Error ? error.message : String(error),
      })
      if (!existingSession) {
        const messagesRecord =
          db.sessionMessages.first({
            where: { sessionId: id },
          }) ?? null
        if (messagesRecord) {
          await Promise.resolve(db.sessionMessages.delete(messagesRecord.id)).catch(() => {})
        }
        const diagnosticsRecord =
          db.sessionDiagnostics.first({
            where: { sessionId: id },
          }) ?? null
        if (diagnosticsRecord) {
          await Promise.resolve(db.sessionDiagnostics.delete(diagnosticsRecord.id)).catch(() => {})
        }
      }
      throw error
    }
  }

  async function newSession(params: NewSessionParams): Promise<DaemonSession> {
    return launchSession(params)
  }

  async function loadSession(params: LoadSessionParams): Promise<DaemonSession> {
    await ready
    const existingRecord = db.sessions.get(params.id) ?? null
    const existingSession = existingRecord ?? null
    if (!existingSession) {
      throw new IpcClientError(`Cannot load unknown session: ${params.id}`)
    }

    return launchSession(params, existingSession)
  }

  async function getSession(id: DaemonSessionId): Promise<DaemonSession> {
    await ready
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw new IpcClientError("Session not found")
    }
    return record
  }

  async function listSessions(
    params: ListDaemonSessionsRequest,
  ): Promise<ListDaemonSessionsResponse> {
    await ready
    const pageSize = normalizeSessionPageSize(params.limit)
    let page: ReturnType<typeof db.sessions.findPage>

    try {
      page = db.sessions.findPage({
        orderBy: {
          updatedAt: "desc",
          id: "desc",
        },
        limit: pageSize,
        after: params.cursor ?? undefined,
      })
    } catch {
      throw new IpcClientError("Invalid session cursor")
    }

    return {
      sessions: page.items,
      nextCursor: page.next ?? null,
      hasMore: page.next != null,
    }
  }

  async function connectSession(id: DaemonSessionId): Promise<DaemonSession> {
    await ready
    if (!activeSessions.has(id)) {
      const session = await getSession(id)
      throw new IpcClientError(
        session.connectionMode === "history"
          ? `Session ${id} is archived and no longer reconnectable`
          : `Session ${id} is not reconnectable`,
      )
    }

    await emitDiagnostic(id, "session_connected")
    return getSession(id)
  }

  async function getHistory(id: DaemonSessionId): Promise<GetDaemonSessionHistoryResponse> {
    await ready
    const active = activeSessions.get(id)
    const session = await getSession(id)
    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      connection: toConnectionState({
        mode: session.connectionMode,
        activeDaemonSession: session.activeDaemonSession,
      }),
      history: active
        ? [...active.history]
        : ((
            db.sessionMessages.first({
              where: { sessionId: id },
            }) ?? null
          )?.messages ?? []),
    }
  }

  async function getDiagnostics(id: DaemonSessionId): Promise<GetDaemonSessionDiagnosticsResponse> {
    await ready
    const session = await getSession(id)
    const diagnosticsRecord =
      db.sessionDiagnostics.first({
        where: { sessionId: id },
      }) ?? null
    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      connection: toConnectionState({
        mode: session.connectionMode,
        activeDaemonSession: session.activeDaemonSession,
      }),
      events: (diagnosticsRecord?.events ?? []).map((event) => ({
        ...event,
        sessionId: session.id,
      })) satisfies DaemonDiagnosticEvent[],
    }
  }

  async function getWorktree(id: DaemonSessionId): Promise<GetDaemonSessionWorktreeResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord =
      db.worktrees.first({
        where: { sessionId: id },
      }) ?? null

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: worktreeRecord
        ? (({ id: _id, sessionId: _sessionId, ...value }) => value)(worktreeRecord)
        : null,
    }
  }

  async function getWorkforce(id: DaemonSessionId): Promise<GetDaemonSessionWorkforceResponse> {
    await ready
    const session = await getSession(id)
    const workforceRecord =
      db.workforces.first({
        where: { sessionId: id },
      }) ?? null

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      workforce: workforceRecord
        ? (({ id: _id, sessionId: _sessionId, ...value }) => value)(workforceRecord)
        : null,
    }
  }

  async function sendMessage(id: DaemonSessionId, message: acp.AnyMessage): Promise<void> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
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

    logAgentMessage("agent.message_write", active.id, active.acpSessionId, message)
    await appendHistory(active.id, message)
    await emitDiagnostic(active.id, "session_message_sent", {
      hasId: "id" in message && message.id != null,
      method: "method" in message ? message.method : undefined,
    })
    input.publish(active.id, message)
    await active.writer.write(message)
  }

  async function promptSession(
    id: DaemonSessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<acp.PromptResponse> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
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
          sessionId: active.acpSessionId,
          prompt: typeof prompt === "string" ? [{ type: "text", text: prompt }] : prompt,
        },
      } satisfies acp.AnyMessage)
      return await response
    } catch (error) {
      active.pendingPrompts.delete(requestId)
      throw error
    }
  }

  async function shutdownSession(id: DaemonSessionId): Promise<boolean> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    await emitDiagnostic(id, "session_shutdown_requested")
    await treeKill(active.process)
    return true
  }

  async function resolveSessionIdByToken(token: string): Promise<DaemonSessionId> {
    await ready
    const record =
      db.sessions.first({
        where: { token },
      }) ?? null
    if (!record?.permissions) {
      throw new IpcClientError("Invalid session token")
    }

    return record.id
  }

  async function close(): Promise<void> {
    await ready
    for (const session of activeSessions.values()) {
      await emitDiagnostic(session.id, "daemon_shutdown", { status: session.status })
      await treeKill(session.process)
      await session.writer.close().catch(() => {})
      await session.subscription.close().catch(() => {})
      const sessionRecord = db.sessions.get(session.id) ?? null
      if (sessionRecord?.permissions) {
        db.sessions.update(session.id, {
          token: null,
          permissions: null,
        })
      }
    }
    activeSessions.clear()
  }

  return {
    newSession,
    loadSession,
    listSessions,
    connectSession,
    getSession,
    getHistory,
    getDiagnostics,
    getWorktree,
    getWorkforce,
    sendMessage,
    promptSession,
    shutdownSession,
    resolveSessionIdByToken,
    close,
  }
}
