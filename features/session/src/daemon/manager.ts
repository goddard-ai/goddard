import { randomBytes, randomUUID } from "node:crypto"
import { readFileSync } from "node:fs"
import treeKill from "@alloc/tree-kill"
import type { AgentService } from "@goddard-ai/agent/daemon"
import { resolveDefaultAgent } from "@goddard-ai/config/node"
import type {
  DaemonAgentEnvironmentService,
  DaemonConfigProvider,
  DaemonLogger,
  DaemonLogService,
  DaemonSessionContext,
  DaemonSessionContextService,
  EventBus,
} from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AttentionMetadataInput } from "@goddard-ai/schema/attention"
import type { AgentsConfig, StaticSessionParams } from "@goddard-ai/schema/config"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"
import {
  createAcpClient,
  type AcpClient,
  type AcpSession,
  type AgentInputStream,
  type AgentOutputStream,
} from "acp-client"
import * as acp from "acp-client/protocol"
import { clamp, getErrorMessage, unique } from "radashi"

import type { SessionDb } from "../daemon.ts"
import type { sessionEvents } from "../events.ts"
import {
  parseSessionIdleShutdownDurationMs,
  SessionErrorCodes,
  type CancelSessionResponse,
  type CreateSessionRequest,
  type DaemonSession,
  type DaemonSessionDiagnosticEvent,
  type DaemonSessionStatus,
  type DaemonSessionTurn,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryRequest,
  type GetSessionHistoryResponse,
  type GetSessionWorktreeResponse,
  type InitialPromptOption,
  type ListSessionsRequest,
  type ListSessionsResponse,
  type PopQueuedSessionPromptResponse,
  type PrepareSessionLaunchWorktreeRequest,
  type PrepareSessionLaunchWorktreeResponse,
  type ReleaseSessionLaunchLeaseRequest,
  type ReleaseSessionLaunchLeaseResponse,
  type ReleaseSessionLaunchWorktreeRequest,
  type ReleaseSessionLaunchWorktreeResponse,
  type SessionComposerSuggestionsRequest,
  type SessionComposerSuggestionsResponse,
  type SessionDraftSuggestionsRequest,
  type SessionLaunchPreviewRequest,
  type SessionLaunchPreviewResponse,
  type SessionLifecycleField,
  type SessionsConfig,
  type SessionSubpackagesRequest,
  type SessionSubpackagesResponse,
  type SessionTitlesConfig,
  type SetSessionConfigOptionRequest,
  type SetSessionModelRequest,
  type SteerSessionResponse,
  type SubpackagesConfig,
  type WorktreesConfig,
} from "../schema.ts"
import { createActiveTurnStore } from "./active-turns.ts"
import {
  spawnAgentProcess,
  waitForAgentProcessExit,
  type AgentProcessHandle,
} from "./agent-process.ts"
import { readSessionChanges } from "./changes.ts"
import {
  getDraftComposerSuggestions,
  getSlashComposerSuggestions,
  normalizeComposerSuggestionLimit,
} from "./composer-suggestions.ts"
import { createIdleShutdownController } from "./idle-shutdown.ts"
import { createSessionIpcError } from "./ipc-error.ts"
import { createLaunchLeaseStore, type LaunchLease } from "./launch-lease.ts"
import { checkoutLocalBranch, createLaunchPreparationFeature } from "./launch-preparation.ts"
import { createLaunchWorktreeFeature } from "./launch-worktrees.ts"
import { createPromptTurnFeature, injectSystemPrompt } from "./prompt-turns.ts"
import { createSessionAttentionFeature } from "./session-attention.ts"
import { createSessionMemory, type ActiveSession } from "./session-memory.ts"
import {
  agentNameFromInput,
  createReconnectRequest,
  createSessionRecordUpdate,
  disconnectedConnectionMode,
  mergeSessionMetadata,
  parseRepoScope,
  persistLaunchedSession,
  resolveExistingSessionArtifacts,
  toConnectionState,
  type ResolvedCreateSessionRequest,
  type SessionConnectionMode,
} from "./session-records.ts"
import { createSessionTitleRuntime } from "./session-titles-runtime.ts"
import {
  createSessionWorktreeFeature,
  toSessionWorktreeLifecycleState,
  type SessionWorktreeLifecycleState,
} from "./session-worktrees.ts"
import { discoverSessionSubpackages } from "./subpackages.ts"
import { backfillSessionTitle, prepareSessionTitle } from "./title.ts"
import {
  createInitializedHistoryTurn,
  getContextUsageFromMessage,
  getLatestAvailableCommands,
  getLatestContextUsage,
  getSessionTurnMessagePayload,
  toSessionHistoryTurnFromActiveTurn,
  toSessionHistoryTurnFromDraft,
  toSessionHistoryTurnFromRecord,
  type SessionTurnPromptRequestId,
} from "./turn-history.ts"
import {
  resolveAvailableWorktreeBranchName,
  resolvePullRequestWorktreeBranchName,
} from "./worktree-branch.ts"
import {
  resolveGitWorktreeSource,
  reuseExistingWorktree,
  toPreparedSessionWorktree,
  type PreparedSessionWorktree,
  type SessionWorktreeState,
} from "./worktree.ts"
import { prepareFreshWorktree } from "./worktrees/bootstrap.ts"
import { createWorktree } from "./worktrees/index.ts"
import { createWorktreePluginManager } from "./worktrees/plugin-manager.ts"
import { defaultPlugin } from "./worktrees/plugins/default.ts"

export { injectSystemPrompt } from "./prompt-turns.ts"
export type { SessionWorktreeLifecycleState } from "./session-worktrees.ts"

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

type SessionId = DaemonSession["id"]

const DEFAULT_IDLE_SESSION_SHUTDOWN_TIMEOUT_MS = 15 * 60 * 1000

/** Daemon session document shape used when reading sessions back from kindstore. */
type SessionDoc = DaemonSession
/** Identifies seeded display fixtures that were never owned by a live daemon process. */
function isMockHistoricalSession(session: SessionDoc) {
  return (
    session.metadata?.mock === true &&
    session.connectionMode !== "live" &&
    session.activeDaemonSession === false &&
    session.supportsLoadSession === false
  )
}

type SessionManagerRootConfig = {
  agents?: AgentsConfig
  session?: StaticSessionParams
  sessions?: SessionsConfig
  sessionTitles?: SessionTitlesConfig
  subpackages?: SubpackagesConfig
  worktrees?: WorktreesConfig
  registry?: Record<string, AgentDistribution>
}

const CMD_DECLARE_INITIATIVE_PROMPT = readTextPrompt("cmd-declare-initiative.md")
const CMD_REPORT_BLOCKER_PROMPT = readTextPrompt("cmd-report-blocker.md")
const FOREGROUND_PROMPT = readTextPrompt("foreground.md")
const GLOBAL_RULES_PROMPT = readTextPrompt("global-rules.md")

/** Reads shared prompt text without relying on bundler-only raw import declarations. */
function readTextPrompt(name: string) {
  return readFileSync(new URL(`./prompts/${name}`, import.meta.url), {
    encoding: "utf-8",
  })
}

/** Applies launch-time ACP model and config-option choices before the first prompt runs. */
async function applyInitialSessionConfiguration(params: {
  session: AcpSession
  configOptions: acp.SessionConfigOption[] | null | undefined
  request: CreateSessionRequest
}) {
  let configOptions = params.configOptions ?? []

  if (params.request.initialModelId) {
    const response = await params.session.setModel(params.request.initialModelId)
    configOptions = response.configOptions
  }

  for (const option of params.request.initialConfigOptions ?? []) {
    // ACP config option values are always string ids, even when the launch form captured
    // a boolean choice locally.
    const response = await params.session.setConfigOption(option.configId, String(option.value))

    configOptions = response.configOptions
  }

  return {
    configOptions,
  }
}

/** Shared session-launch options resolved by the daemon before an agent process starts. */
export interface SessionLaunchParams {
  request: CreateSessionRequest
  token?: string
  config?: SessionManagerRootConfig
  worktreePlugins?: WorktreePlugin[]
  onPersisted?: (input: { sessionId: SessionId }) => void | Promise<void>
}

/** Fresh daemon session input accepted by `SessionManager.newSession()`. */
export interface NewSessionParams extends SessionLaunchParams {}

/** Stored daemon session input accepted by `SessionManager.loadSession()`. */
export interface LoadSessionParams extends SessionLaunchParams {
  id: SessionId
}

/** Exposes the inferred daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = ReturnType<typeof createSessionManager>

function createInitialPromptRequest(params: {
  sessionId: string
  prompt: InitialPromptOption
  isFirstPrompt: boolean
  systemPrompt: string
}) {
  const promptRequest: acp.PromptRequest = {
    sessionId: params.sessionId,
    prompt:
      typeof params.prompt === "string" ? [{ type: "text", text: params.prompt }] : params.prompt,
  }

  return params.isFirstPrompt
    ? injectSystemPrompt(promptRequest, params.systemPrompt)
    : promptRequest
}

/** Treats abrupt termination signals as session errors instead of normal shutdowns. */
function isErrorSignal(signal: string | null): boolean {
  return signal === "SIGKILL" || signal === "SIGABRT" || signal === "SIGQUIT"
}

/** Detects one-shot sessions that should exit immediately after the initial prompt completes. */
function shouldExitAfterInitialPrompt(params: SessionLaunchParams): boolean {
  return params.request.oneShot === true && params.request.initialPrompt !== undefined
}

function renderPrompt(template: string, variables: Record<string, string>): string {
  return template.replace(/\${(\w+)}/g, (_, key) => {
    const value = variables[key]
    if (typeof value !== "string") {
      throw new Error(`Prompt variable "${key}" is not a string`)
    }

    return value
  })
}

function buildForegroundSystemPrompt() {
  return renderPrompt(FOREGROUND_PROMPT, {
    declare_initiative: CMD_DECLARE_INITIATIVE_PROMPT,
    report_blocker: CMD_REPORT_BLOCKER_PROMPT,
    global_rules: GLOBAL_RULES_PROMPT,
  })
}

function resolveSystemPrompt(request: CreateSessionRequest) {
  if (request.systemPrompt !== undefined) {
    return request.systemPrompt
  }

  if (request.oneShot === true) {
    return ""
  }

  return buildForegroundSystemPrompt()
}

/** Returns true when the ACP adapter can reopen this session later via `session/load`. */
function supportsSessionLoad(initialized: Pick<InitializedSession, "agentCapabilities">): boolean {
  return initialized.agentCapabilities?.loadSession === true
}

type InitializedSession = acp.InitializeResponse & {
  client: AcpClient
  session: AcpSession
  status: DaemonSessionStatus
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  initialPromptRequestId: SessionTurnPromptRequestId | null
  initialPromptStartedAt: string | null
  initialPromptCompletedAt: string | null
  acpSessionId: string
  configOptions?: acp.SessionConfigOption[] | null
  stopReason: acp.PromptResponse["stopReason"] | null
}

/** Runs an optional launch-time prompt and captures the synthetic turn history around it. */
async function runLaunchInitialPrompt(params: {
  session: AcpSession
  acpSessionId: string
  request: ResolvedCreateSessionRequest
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  onMessageWrite?: (message: acp.AnyMessage) => void
}) {
  let status: DaemonSessionStatus = "active"
  let isFirstPrompt = params.isFirstPrompt
  let initialPromptRequestId: SessionTurnPromptRequestId | null = null
  let initialPromptStartedAt: string | null = null
  let initialPromptCompletedAt: string | null = null
  let stopReason: acp.PromptResponse["stopReason"] | null = null

  if (params.request.initialPrompt !== undefined) {
    initialPromptRequestId = randomUUID()
    initialPromptStartedAt = new Date().toISOString()
    const initialMessage = {
      jsonrpc: "2.0",
      id: initialPromptRequestId,
      method: acp.AGENT_METHODS.session_prompt,
      params: createInitialPromptRequest({
        sessionId: params.acpSessionId,
        prompt: params.request.initialPrompt,
        isFirstPrompt,
        systemPrompt: params.request.systemPrompt,
      }),
    } satisfies acp.AnyMessage

    params.history.push(initialMessage)
    params.onMessageWrite?.(initialMessage)

    const response = await params.session.prompt(initialMessage.params.prompt)
    initialPromptCompletedAt = new Date().toISOString()
    params.history.push({
      jsonrpc: "2.0",
      id: initialPromptRequestId,
      result: response,
    } satisfies acp.AnyMessage)
    stopReason = response.stopReason

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
    status,
    isFirstPrompt,
    initialPromptRequestId,
    initialPromptStartedAt,
    initialPromptCompletedAt,
    stopReason,
  }
}

/** Performs the ACP handshake and optional initial prompt before live streaming begins. */
async function initializeSession(params: {
  input: AgentInputStream
  output: AgentOutputStream
  request: ResolvedCreateSessionRequest
  resumeAcpId?: string
  onMessageWrite?: (message: acp.AnyMessage) => void
  findActiveSession?: (acpSessionId: string) => ActiveSession | null
  handleSessionUpdate?: (active: ActiveSession, params: acp.SessionNotification) => Promise<void>
  handlePermissionRequest?: (
    active: ActiveSession,
    params: acp.RequestPermissionRequest,
  ) => Promise<acp.RequestPermissionResponse>
}): Promise<InitializedSession> {
  const history: acp.AnyMessage[] = []
  let routeAcpSessionId: string | null = null
  const client = await createAcpClient({
    stdin: params.input,
    stdout: params.output,
    clientInfo: {
      name: "npm:@goddard-ai/daemon",
      version: getPackageVersion(),
    },
    handler: {
      async requestPermission(permissionParams) {
        const active =
          params.findActiveSession?.(permissionParams.sessionId) ??
          (routeAcpSessionId ? (params.findActiveSession?.(routeAcpSessionId) ?? null) : null)
        if (active && params.handlePermissionRequest) {
          return await params.handlePermissionRequest(active, permissionParams)
        }

        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(updateParams) {
        const active =
          params.findActiveSession?.(updateParams.sessionId) ??
          (routeAcpSessionId ? (params.findActiveSession?.(routeAcpSessionId) ?? null) : null)
        if (active && params.handleSessionUpdate) {
          await params.handleSessionUpdate(active, updateParams)
          return
        }

        history.push({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params: updateParams,
        })
      },
    },
  })

  try {
    const initializeResult = client.initialize

    let isFirstPrompt = true
    let acpSessionId: string
    let session: AcpSession
    let configOptions: acp.SessionConfigOption[] | null | undefined

    if (params.resumeAcpId !== undefined) {
      if (initializeResult.agentCapabilities?.loadSession !== true) {
        throw createSessionIpcError(SessionErrorCodes.CannotResumeUnsupportedAgent, {
          acpSessionId: params.resumeAcpId,
        })
      }

      session = await client.loadSession({
        sessionId: params.resumeAcpId,
        cwd: params.request.cwd,
        mcpServers: params.request.mcpServers,
      })
      acpSessionId = params.resumeAcpId
      routeAcpSessionId = acpSessionId
      isFirstPrompt = false
    } else {
      session = await client.newSession(params.request)
      acpSessionId = session.sessionId
      routeAcpSessionId = acpSessionId
      configOptions = session.configOptions

      if (
        params.request.initialModelId !== undefined ||
        (params.request.initialConfigOptions?.length ?? 0) > 0
      ) {
        const configuredSession = await applyInitialSessionConfiguration({
          session,
          configOptions,
          request: params.request,
        })

        configOptions = configuredSession.configOptions
      }
    }

    const initialPromptResult = await runLaunchInitialPrompt({
      session,
      acpSessionId,
      request: params.request,
      isFirstPrompt,
      history,
      onMessageWrite: params.onMessageWrite,
    })

    return {
      ...initializeResult,
      ...initialPromptResult,
      client,
      session,
      history,
      acpSessionId,
      configOptions,
    }
  } catch (error) {
    await client.close()
    throw error
  }
}

/** Promotes one prepared launch lease by applying final launch options and optional initial prompt. */
async function initializeSessionFromLaunchLease(params: {
  lease: LaunchLease
  request: ResolvedCreateSessionRequest
  onMessageWrite?: (message: acp.AnyMessage) => void
}) {
  let configOptions: acp.SessionConfigOption[] | null | undefined = params.lease.configOptions

  if (
    params.request.initialModelId !== undefined ||
    (params.request.initialConfigOptions?.length ?? 0) > 0
  ) {
    const configuredSession = await applyInitialSessionConfiguration({
      session: params.lease.session,
      configOptions,
      request: params.request,
    })

    configOptions = configuredSession.configOptions
  }

  const initialPromptResult = await runLaunchInitialPrompt({
    session: params.lease.session,
    acpSessionId: params.lease.acpSessionId,
    request: params.request,
    isFirstPrompt: true,
    history: params.lease.history,
    onMessageWrite: params.onMessageWrite,
  })

  return {
    ...params.lease.initializeResult,
    ...initialPromptResult,
    client: params.lease.client,
    session: params.lease.session,
    history: params.lease.history,
    acpSessionId: params.lease.acpSessionId,
    configOptions,
  } satisfies InitializedSession
}

/**
 * Returns true when one launch path needs configured worktree plugins for reuse or creation.
 */
function shouldResolveConfiguredWorktreePlugins(
  request: CreateSessionRequest,
  existingWorktree: SessionWorktreeState | null,
) {
  return existingWorktree !== null || request.worktree?.enabled === true
}

/** Resolves the effective worktree used by one session launch, either by reuse or fresh creation. */
async function resolveLaunchWorktree(params: {
  request: CreateSessionRequest
  existingWorktree: SessionWorktreeState | null
  preparedLaunchWorktree: PreparedSessionWorktree | null
  worktreePlugins?: WorktreePlugin[]
  defaultWorktreesFolder?: string
  branchPrefix?: string
}) {
  if (params.existingWorktree) {
    await reuseExistingWorktree(params.existingWorktree, {
      worktreePlugins: params.worktreePlugins,
    })
    return toPreparedSessionWorktree(params.existingWorktree)
  }

  if (params.preparedLaunchWorktree) {
    return params.preparedLaunchWorktree
  }

  const source = await resolveGitWorktreeSource(params.request.cwd)

  if (params.request.worktree?.enabled !== true) {
    if (source?.bare) {
      throw createSessionIpcError(SessionErrorCodes.LaunchBareRepository, {
        cwd: params.request.cwd,
      })
    }

    return null
  }

  if (!source) {
    return null
  }

  return toPreparedSessionWorktree(
    await createWorktree({
      cwd: source.path,
      requestedCwd: params.request.cwd,
      branchName:
        typeof params.request.prNumber === "number"
          ? resolvePullRequestWorktreeBranchName({
              repository: params.request.repository,
              prNumber: params.request.prNumber,
            })
          : await resolveAvailableWorktreeBranchName({
              cwd: source.path,
              branchPrefix: params.branchPrefix,
            }),
      baseBranchName: params.request.worktree?.baseBranchName,
      plugins: params.worktreePlugins,
      defaultPluginDirName: params.defaultWorktreesFolder,
    }),
  )
}

/** Builds the structured logging context shared across session lifecycle events. */
function buildSessionLogContext(params: {
  request: ResolvedCreateSessionRequest
  cwd?: string
  extraContext?: Record<string, unknown>
}): Record<string, unknown> {
  return {
    agent: agentNameFromInput(params.request.agent),
    cwd: params.cwd ?? params.request.cwd,
    oneShot: params.request.oneShot === true,
    repository:
      typeof params.request.repository === "string" ? params.request.repository : undefined,
    prNumber: typeof params.request.prNumber === "number" ? params.request.prNumber : undefined,
    ...params.extraContext,
  }
}

/** Builds the stable async context installed while one daemon session is actively doing work. */
function buildSessionContext(params: {
  sessionId: SessionId
  request: ResolvedCreateSessionRequest
  cwd: string
  worktree?: PreparedSessionWorktree | null
}) {
  const sessionContext: DaemonSessionContext = {
    sessionId: params.sessionId,
    acpSessionId: null,
    cwd: params.cwd,
    repository: typeof params.request.repository === "string" ? params.request.repository : null,
    prNumber: typeof params.request.prNumber === "number" ? params.request.prNumber : null,
    worktreeDir: params.worktree?.state.worktreeDir ?? null,
    worktreePoweredBy: params.worktree?.state.poweredBy ?? null,
  }

  return sessionContext
}

/** Resolves the concrete launch agent so daemon session creation never depends on client fallback logic. */
async function resolveSessionRequestAgent(
  request: CreateSessionRequest,
  config?: SessionManagerRootConfig,
): Promise<ResolvedCreateSessionRequest> {
  return {
    ...request,
    agent: request.agent ?? (await resolveDefaultAgent(config)),
    systemPrompt: resolveSystemPrompt(request),
  }
}

/** Rejects any in-flight prompt waits when a daemon session is torn down. */
function rejectPendingPrompts(active: ActiveSession, error: Error): void {
  for (const queued of active.promptQueue) {
    queued.reject?.(error)
  }
  active.promptQueue.length = 0
  if (active.lastPermissionRequest) {
    active.lastPermissionRequest.resolve({ outcome: { outcome: "cancelled" } })
    active.lastPermissionRequest = null
  }
  if (active.pendingSteer) {
    active.pendingSteer.reject(error)
    active.pendingSteer = null
  }
}

const DEFAULT_SESSION_PAGE_SIZE = 20
const MAX_SESSION_PAGE_SIZE = 100

export type SessionEventEmitter = EventBus<typeof sessionEvents>

/** Normalizes optional session page sizes to the daemon's supported bounds. */
function normalizeSessionPageSize(limit?: number): number {
  if (!Number.isFinite(limit)) {
    return DEFAULT_SESSION_PAGE_SIZE
  }

  return clamp(Math.trunc(limit ?? DEFAULT_SESSION_PAGE_SIZE), 1, MAX_SESSION_PAGE_SIZE)
}

/** Creates the daemon-owned session lifecycle boundary over storage and agent processes. */
export function createSessionManager({
  db,
  getDaemonUrl,
  createAgentEnvironment,
  events,
  configProvider,
  log,
  agentService,
  sessionContext: sessionContextService,
  idleSessionShutdownTimeoutMs,
}: {
  db: SessionDb
  getDaemonUrl: () => string
  createAgentEnvironment: DaemonAgentEnvironmentService["createAgentEnvironment"]
  events: SessionEventEmitter
  configProvider: DaemonConfigProvider<SessionManagerRootConfig>
  log: DaemonLogService
  agentService: AgentService
  sessionContext: DaemonSessionContextService
  idleSessionShutdownTimeoutMs?: number
}) {
  const logger = log.createLogger()
  const memory = createSessionMemory()
  const activeSessions = memory.activeSessions
  const activeSessionsByAcpSessionId = memory.activeSessionsByAcpSessionId
  const launchLeaseStore = createLaunchLeaseStore({ logger })
  const worktreePluginManager = createWorktreePluginManager({
    configProvider,
    logger,
  })
  const idleShutdown = createIdleShutdownController({
    memory,
    logger,
    emitDiagnostic,
    shutdownSession,
  })
  const acpDebug = log.createDebug("session.acp")
  const activeTurns = createActiveTurnStore({
    db,
    debug: log.createDebug("session.turns"),
    emitDiagnostic,
    publishSessionUpdated,
    refreshIdleShutdownState: idleShutdown.refreshIdleShutdownState,
    updateSessionAvailableCommands,
    updateSessionContextUsage,
  })
  const sessionTitles = createSessionTitleRuntime({
    db,
    memory,
    configProvider,
    debug: log.createDebug("session.titles"),
    emitDiagnostic,
    updateSession,
  })
  const sessionAttention = createSessionAttentionFeature({
    db,
    memory,
    events,
    updateSessionActivity,
  })
  const sessionWorktrees = createSessionWorktreeFeature({
    db,
    memory,
    events,
    updateSessionActivity,
  })
  const launchWorktrees = createLaunchWorktreeFeature({
    db,
    configProvider,
    logger,
    worktreePluginManager,
  })
  const promptTurns = createPromptTurnFeature({
    db,
    memory,
    log,
    events,
    activeTurns,
    idleShutdown,
    sessionTitles,
    emitDiagnostic,
    publishSessionUpdated,
    updateSessionActivity,
  })
  const launchPreparation = createLaunchPreparationFeature({
    memory,
    launchLeaseStore,
    configProvider,
    getDaemonUrl,
    createAgentEnvironment,
    agentService,
    getPackageVersion,
    handlePermissionRequest: promptTurns.handlePermissionRequest,
    handleSessionUpdate: promptTurns.handleSessionUpdate,
  })
  const ready = (async () => {
    await launchWorktrees.cleanupColdWorktrees()
    await reconcilePersistedSessions()
  })()

  function publishSessionUpdated(id: SessionId, changed: readonly SessionLifecycleField[]) {
    const session = db.sessions.get(id) ?? null
    if (!session || changed.length === 0) {
      return
    }

    void events.emit("session.lifecycle.updated", {
      kind: "sessionUpdated",
      session,
      changed: unique(changed),
    })
  }

  function lifecycleFieldsFromSessionUpdate(update: Partial<DaemonSession>) {
    const fields: SessionLifecycleField[] = []

    if (update.status !== undefined) {
      fields.push("status")
    }
    if (update.connectionMode !== undefined || update.activeDaemonSession !== undefined) {
      fields.push("connection")
    }
    if (update.title !== undefined || update.titleState !== undefined) {
      fields.push("title")
    }
    if (update.contextUsage !== undefined) {
      fields.push("contextUsage")
    }
    if (update.lastAgentMessage !== undefined) {
      fields.push("lastAgentMessage")
    }
    if (update.lastSessionActivityAt !== undefined) {
      fields.push("lastSessionActivity")
    }
    if (update.completedHidden !== undefined) {
      fields.push("completedHidden")
    }

    return fields
  }

  /** Resolves the idle shutdown timeout after root config has been resolved for a session cwd. */
  function resolveIdleSessionShutdownTimeoutMs(config?: SessionManagerRootConfig): number {
    if (idleSessionShutdownTimeoutMs !== undefined) {
      return idleSessionShutdownTimeoutMs
    }

    const configuredDuration = config?.sessions?.idleShutdown
    if (configuredDuration) {
      const parsedDuration = parseSessionIdleShutdownDurationMs(configuredDuration)
      if (parsedDuration !== null) {
        return parsedDuration
      }
    }

    return DEFAULT_IDLE_SESSION_SHUTDOWN_TIMEOUT_MS
  }

  function updateSession(
    id: SessionId,
    update: Partial<DaemonSession>,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) {
    const active = activeSessions.get(id)
    const previousRecord = db.sessions.get(id) ?? null
    const previousStatus = active?.status ?? previousRecord?.status
    const resolvedLogger = diagnosticLogger ?? active?.logger ?? logger
    if (update.status && active) {
      active.status = update.status
    }

    if (previousRecord) {
      db.sessions.update(previousRecord.id, update)
      publishSessionUpdated(id, lifecycleFieldsFromSessionUpdate(update))
    }
    if (update.status && previousStatus && previousStatus !== update.status) {
      emitDiagnostic(
        id,
        "session_status_changed",
        {
          previousStatus,
          nextStatus: update.status,
          ...detail,
        },
        resolvedLogger,
      )
    }
  }

  function updateSessionActivity(
    id: SessionId,
    update: Partial<DaemonSession>,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) {
    updateSession(
      id,
      {
        ...update,
        lastSessionActivityAt: Date.now(),
      },
      detail,
      diagnosticLogger,
    )
  }

  function requireSessionDocument(id: SessionId) {
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw createSessionIpcError(SessionErrorCodes.NotFound, {
        sessionId: id,
      })
    }

    return record
  }

  function updateSessionAvailableCommands(
    sessionId: SessionId,
    availableCommands: acp.AvailableCommand[],
  ) {
    const sessionRecord = db.sessions.get(sessionId) ?? null
    if (!sessionRecord) {
      return
    }

    db.sessions.update(sessionId, {
      availableCommands,
    })
  }

  function updateSessionContextUsage(sessionId: SessionId, message: acp.AnyMessage) {
    const contextUsage = getContextUsageFromMessage(message)
    if (!contextUsage || !db.sessions.get(sessionId)) {
      return false
    }

    db.sessions.update(sessionId, {
      contextUsage,
    })
    publishSessionUpdated(sessionId, ["contextUsage"])
    return true
  }

  function hasPersistedTurnHistory(sessionId: SessionId) {
    return (
      db.sessionTurns.first({
        where: { sessionId },
      }) != null ||
      db.sessionTurnDrafts.first({
        where: { sessionId },
      }) != null
    )
  }

  function emitDiagnostic(
    sessionId: SessionId,
    type: string,
    detail?: Record<string, unknown>,
    diagnosticLogger: DaemonLogger = logger,
    options: { logSessionId?: boolean } = {},
  ) {
    const event: DaemonSessionDiagnosticEvent = {
      type,
      at: new Date().toISOString(),
      detail,
    }
    diagnosticLogger.log(type, {
      ...(options.logSessionId === false ? {} : { sessionId }),
      ...detail,
    })
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

  function setConnectionMode(
    sessionId: SessionId,
    mode: SessionConnectionMode,
    activeDaemonSession: boolean,
  ) {
    const sessionRecord = db.sessions.get(sessionId) ?? null
    if (!sessionRecord) {
      return
    }

    db.sessions.update(sessionRecord.id, {
      connectionMode: mode,
      activeDaemonSession,
    })
    publishSessionUpdated(sessionId, ["connection"])
  }

  /** Records one new `session.message event stream` subscriber so idle shutdown waits for attached clients. */
  async function sessionSubscriberConnected(id: SessionId): Promise<void> {
    await ready
    idleShutdown.sessionSubscriberConnected(id)
  }

  /** Records one departing `session.message event stream` subscriber and starts the timer when none remain. */
  async function sessionSubscriberDisconnected(id: SessionId): Promise<void> {
    await ready
    idleShutdown.sessionSubscriberDisconnected(id)
  }

  async function reconcilePersistedSessions(): Promise<void> {
    let persistedSessions: SessionDoc[]

    try {
      persistedSessions = await Promise.resolve(db.sessions.findMany())
    } catch (error) {
      logger.log("session_reconciliation_failed", {
        errorMessage: getErrorMessage(error),
      })
      return
    }

    await Promise.all(
      persistedSessions.map(async (session) => {
        const supportsLoadSession = session.supportsLoadSession === true
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

        const draftRecord =
          db.sessionTurnDrafts.first({
            where: { sessionId: session.id },
          }) ?? null
        const titleBackfill = backfillSessionTitle({
          title: session.title,
          titleState: session.titleState,
          initiative: session.initiative,
          history: [
            ...db.sessionTurns
              .findMany({
                where: { sessionId: session.id },
              })
              .sort(
                (left: DaemonSessionTurn, right: DaemonSessionTurn) =>
                  left.sequence - right.sequence,
              )
              .flatMap((turn: DaemonSessionTurn) =>
                turn.messages.map(getSessionTurnMessagePayload),
              ),
            ...(draftRecord?.messages.map(getSessionTurnMessagePayload) ?? []),
          ],
        })
        if (titleBackfill) {
          const sessionDocument = db.sessions.get(session.id) ?? null
          if (sessionDocument) {
            db.sessions.update(session.id, titleBackfill)
          }

          if (session.titleState === "pending" && titleBackfill.titleState === "failed") {
            emitDiagnostic(session.id, "session_title_generation_failed", {
              reason: "daemon_reconciliation",
            })
          }
        }
        if (draftRecord) {
          activeTurns.persistTurnDraftAsInterruptedTurn(session.id, draftRecord, logger)
        }

        if (
          !isMockHistoricalSession(session) &&
          (session.status === "active" || session.status === "blocked" || session.status === "idle")
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
          setConnectionMode(
            session.id,
            disconnectedConnectionMode(hasPersistedTurnHistory(session.id), supportsLoadSession),
            false,
          )
          emitDiagnostic(session.id, "session_reconciled_after_restart", {
            previousStatus: session.status,
          })
          return
        }

        setConnectionMode(
          session.id,
          disconnectedConnectionMode(hasPersistedTurnHistory(session.id), supportsLoadSession),
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

  async function completeOneShotLaunch(params: {
    id: SessionId
    initialized: InitializedSession
    agentProcess: AgentProcessHandle
    sessionLogger: DaemonLogger
    supportsLoadSession: boolean
  }) {
    params.agentProcess.onceExit((code, signal) => {
      emitDiagnostic(
        params.id,
        "agent_process_exit",
        {
          code,
          signal,
          nextStatus: "done",
        },
        params.sessionLogger,
      )
    })

    updateSessionActivity(
      params.id,
      { status: "done", token: null, permissions: null },
      { reason: "one_shot_completed" },
      params.sessionLogger,
    )
    setConnectionMode(
      params.id,
      disconnectedConnectionMode(true, params.supportsLoadSession),
      false,
    )
    emitDiagnostic(params.id, "session_completed_one_shot", undefined, params.sessionLogger)
    await params.initialized.client.close().catch(() => {})
    await treeKill(params.agentProcess)
    await waitForAgentProcessExit(params.agentProcess)

    const sessionDocument = db.sessions.get(params.id) ?? null
    if (!sessionDocument) {
      throw createSessionIpcError(SessionErrorCodes.NotFound, {
        sessionId: params.id,
      })
    }

    return sessionDocument
  }

  async function activateLiveSession(params: {
    id: SessionId
    token: string
    supportsLoadSession: boolean
    agentProcess: AgentProcessHandle
    initialized: InitializedSession
    nextTurnSequence: number
    sessionLogger: DaemonLogger
    systemPrompt: string
    idleShutdownTimeoutMs: number
  }) {
    const activeSession: ActiveSession = {
      id: params.id,
      acpSessionId: params.initialized.acpSessionId,
      logger: params.sessionLogger,
      token: params.token,
      supportsLoadSession: params.supportsLoadSession,
      process: params.agentProcess,
      client: params.initialized.client,
      session: params.initialized.session,
      status: params.initialized.status,
      exitCleanup: null,
      nextTurnSequence: params.nextTurnSequence,
      activeTurn: null,
      isFirstPrompt: params.initialized.isFirstPrompt,
      systemPrompt: params.systemPrompt,
      lastPermissionRequest: null,
      promptQueue: [],
      blockingPromptRequestId: null,
      pendingSteer: null,
      idleShutdownTimeoutMs: params.idleShutdownTimeoutMs,
      idleShutdownTimer: null,
    }

    const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
      try {
        activeTurns.flushActiveTurnDraft(activeSession, "agent_process_exit")
      } catch {}
      idleShutdown.cancelIdleShutdownTimer(activeSession, "agent_process_exit")
      activeSessions.delete(activeSession.id)
      activeSessionsByAcpSessionId.delete(activeSession.acpSessionId)
      rejectPendingPrompts(
        activeSession,
        new Error(`Session ${activeSession.id} ended before the prompt completed.`),
      )
      await activeSession.client.close().catch(() => {})

      const worktreeRecord = await sessionWorktrees.resolvePersistedWorktreeRecord(activeSession.id)
      try {
        await events.emit("session.stopping", {
          sessionId: activeSession.id,
          reason: "agent_process_exit",
          worktree: worktreeRecord
            ? toSessionWorktreeLifecycleState(worktreeRecord, activeSession.id)
            : null,
        })
      } catch (error) {
        emitDiagnostic(
          activeSession.id,
          "session_stop_cleanup_failed",
          { reason: "agent_process_exit", errorMessage: getErrorMessage(error) },
          activeSession.logger,
        )
      }

      const nextUpdate: Partial<DaemonSession> = {}
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

      setConnectionMode(
        activeSession.id,
        disconnectedConnectionMode(
          hasPersistedTurnHistory(activeSession.id),
          activeSession.supportsLoadSession,
        ),
        false,
      )
      emitDiagnostic(
        activeSession.id,
        "agent_process_exit",
        {
          code,
          signal,
          nextStatus: nextUpdate.status ?? activeSession.status,
        },
        activeSession.logger,
      )
      if (Object.keys(nextUpdate).length > 0) {
        try {
          const persistSessionExit = nextUpdate.status ? updateSessionActivity : updateSession
          persistSessionExit(activeSession.id, nextUpdate, {
            reason: "agent_process_exit",
            code,
            signal,
          })
        } catch {}
      }
    }

    params.agentProcess.onceExit((code, signal) => {
      activeSession.exitCleanup = handleExit(code, signal).catch((error) => {
        emitDiagnostic(
          activeSession.id,
          "session_stop_cleanup_failed",
          { reason: "agent_process_exit", errorMessage: getErrorMessage(error) },
          activeSession.logger,
        )
      })
    })

    activeSessions.set(activeSession.id, activeSession)
    activeSessionsByAcpSessionId.set(activeSession.acpSessionId, activeSession)
    idleShutdown.refreshIdleShutdownState(activeSession.id, "session_activated")
    const sessionDocument = db.sessions.get(params.id) ?? null
    if (!sessionDocument) {
      throw createSessionIpcError(SessionErrorCodes.NotFound, {
        sessionId: params.id,
      })
    }

    return sessionDocument
  }

  async function launchSession(
    params: SessionLaunchParams,
    existingSession: SessionDoc | null = null,
  ): Promise<DaemonSession> {
    await ready
    const id = existingSession?.id ?? db.sessions.newId()
    let token = params.token ?? randomBytes(32).toString("hex")
    const exitAfterInitialPrompt = shouldExitAfterInitialPrompt(params)
    const existingArtifacts = resolveExistingSessionArtifacts(db, id, existingSession)
    const resolvedConfig =
      params.config ??
      (configProvider ? (await configProvider.getRootConfig(params.request.cwd)).config : undefined)
    const resolvedWorktreePlugins =
      params.worktreePlugins ??
      (shouldResolveConfiguredWorktreePlugins(params.request, existingArtifacts.worktree)
        ? await worktreePluginManager.getPlugins(params.request.cwd)
        : undefined)
    const resolvedRequest = await resolveSessionRequestAgent(params.request, resolvedConfig)
    const preparedTitle = prepareSessionTitle(
      resolvedRequest.initialPrompt,
      resolvedConfig?.sessionTitles?.generator,
    )
    const launchWorktree =
      existingArtifacts.worktree === null
        ? launchWorktrees.takeCompatible({
            launchWorktreeId: resolvedRequest.launchWorktreeId,
            request: resolvedRequest,
          })
        : null
    const deferredInitialPrompt = !exitAfterInitialPrompt
      ? resolvedRequest.initialPrompt
      : undefined
    const initializationRequest =
      deferredInitialPrompt === undefined
        ? resolvedRequest
        : {
            ...resolvedRequest,
            initialPrompt: undefined,
          }
    const resolvedRegistry = resolvedConfig?.registry
    const worktree = await resolveLaunchWorktree({
      request: resolvedRequest,
      existingWorktree: existingArtifacts.worktree,
      preparedLaunchWorktree: launchWorktree,
      worktreePlugins: resolvedWorktreePlugins,
      defaultWorktreesFolder: resolvedConfig?.worktrees?.defaultFolder,
      branchPrefix: resolvedConfig?.worktrees?.branchPrefix,
    })
    const cwd = worktree?.state.effectiveCwd ?? resolvedRequest.cwd
    const sessionMetadata = mergeSessionMetadata(
      existingSession?.metadata,
      resolvedRequest.metadata,
    )
    const sessionContext = buildSessionContext({
      sessionId: id,
      request: resolvedRequest,
      cwd,
      worktree,
    })

    const sessionLogContext = buildSessionLogContext({
      request: resolvedRequest,
      cwd,
      extraContext: worktree
        ? {
            worktreeDir: worktree.state.worktreeDir,
            worktreePoweredBy: worktree.state.poweredBy,
          }
        : undefined,
    })

    const scope = parseRepoScope(resolvedRequest)
    const nextPermission = {
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    }

    let sessionLogger = logger
    sessionLogger = sessionContextService.run(sessionContext, () => sessionLogger.snapshot())
    let spawnedAgentProcess: AgentProcessHandle | null = null
    let initializedClient: AcpClient | null = null

    try {
      sessionLogger.log("session.launch_requested", {
        sessionId: id,
        ...sessionLogContext,
      })

      if (
        worktree &&
        !existingArtifacts.worktree &&
        !launchWorktree &&
        worktree.state.poweredBy === defaultPlugin.name
      ) {
        try {
          await prepareFreshWorktree({
            repoRoot: worktree.state.repoRoot,
            worktreeDir: worktree.state.worktreeDir,
            config: resolvedConfig?.worktrees?.bootstrap,
            onEvent: (event) => {
              emitDiagnostic(id, event.type, event.detail, sessionLogger)
            },
          })
        } catch (error) {
          emitDiagnostic(
            id,
            "worktree.bootstrap_failed",
            {
              errorMessage: getErrorMessage(error),
            },
            sessionLogger,
          )
          throw error
        }
      } else if (worktree && existingArtifacts.worktree) {
        emitDiagnostic(
          id,
          "worktree.bootstrap_skipped",
          { reason: "reused_worktree" },
          sessionLogger,
        )
      } else if (worktree && launchWorktree) {
        emitDiagnostic(
          id,
          "worktree.bootstrap_skipped",
          { reason: "prewarmed_worktree" },
          sessionLogger,
        )
      } else if (worktree && worktree.state.poweredBy !== defaultPlugin.name) {
        emitDiagnostic(
          id,
          "worktree.bootstrap_skipped",
          {
            reason: "unsupported_plugin",
            poweredBy: worktree.state.poweredBy,
          },
          sessionLogger,
        )
      }

      if (worktree) {
        await events.emit("session.worktree.prepared", {
          sessionId: id,
          request: resolvedRequest,
          worktree: toSessionWorktreeLifecycleState(worktree.state, id),
        })
      }

      const onMessageWrite = (message: acp.AnyMessage) => {
        acpDebug("session.acp.message_write", {
          sessionId: id,
          acpSessionId: sessionContext.acpSessionId,
          hasId: "id" in message && message.id != null,
          method: "method" in message ? message.method : undefined,
          message: log.createPayloadPreview(message, { maxStringLength: 160 }),
        })
      }

      if (!worktree && resolvedRequest.localCheckout) {
        await checkoutLocalBranch({
          cwd,
          branchName: resolvedRequest.localCheckout.branchName,
        })
      }

      const launchLease = launchLeaseStore.takeCompatible({
        launchLeaseId: resolvedRequest.launchLeaseId,
        request: resolvedRequest,
        cwd,
        existingSession,
        worktree,
      })
      let agentProcess: AgentProcessHandle
      let initialized: InitializedSession

      if (launchLease) {
        token = launchLease.token
        agentProcess = launchLease.agentProcess
        spawnedAgentProcess = agentProcess
        initialized = await initializeSessionFromLaunchLease({
          lease: launchLease,
          request: {
            ...initializationRequest,
            cwd,
            launchLeaseId: undefined,
            localCheckout: undefined,
            metadata: sessionMetadata,
          },
          onMessageWrite,
        })
        initializedClient = initialized.client
      } else {
        agentProcess = await spawnAgentProcess({
          daemonUrl: getDaemonUrl(),
          token,
          agent: resolvedRequest.agent,
          cwd,
          createAgentEnvironment: createAgentEnvironment,
          env: resolvedRequest.env,
          envPolicy: resolvedConfig?.sessions?.envPolicy,
          agentService,
          registry: resolvedRegistry,
          managedAgents: resolvedConfig?.agents?.managed,
        })
        spawnedAgentProcess = agentProcess

        initialized = await initializeSession({
          input: agentProcess.stdin,
          output: agentProcess.stdout,
          request: {
            ...initializationRequest,
            cwd,
            launchLeaseId: undefined,
            localCheckout: undefined,
            metadata: sessionMetadata,
          },
          resumeAcpId: existingSession?.acpSessionId,
          onMessageWrite,
          findActiveSession: (acpSessionId) =>
            activeSessionsByAcpSessionId.get(acpSessionId) ?? null,
          handleSessionUpdate: promptTurns.handleSessionUpdate,
          handlePermissionRequest: promptTurns.handlePermissionRequest,
        })
        initializedClient = initialized.client
      }
      sessionContext.acpSessionId = initialized.acpSessionId

      const latestAvailableCommands = getLatestAvailableCommands(initialized.history)
      const latestContextUsage = getLatestContextUsage(initialized.history)
      const availableCommands = latestAvailableCommands ?? existingSession?.availableCommands ?? []
      const sessionSupportsLoad = supportsSessionLoad(initialized)
      const initialTurn = createInitializedHistoryTurn({
        initialized,
        sequence: existingArtifacts.nextTurnSequence,
      })
      const nextTurnSequence = initialTurn
        ? initialTurn.sequence + 1
        : existingArtifacts.nextTurnSequence
      const sessionRecord = createSessionRecordUpdate({
        initialized,
        request: resolvedRequest,
        cwd,
        token,
        scope,
        nextPermission,
        sessionMetadata,
        existingSession,
        exitAfterInitialPrompt,
        supportsLoadSession: sessionSupportsLoad,
        title: preparedTitle.title,
        titleState: preparedTitle.titleState,
        availableCommands,
        contextUsage: latestContextUsage,
      })

      persistLaunchedSession(db, {
        id,
        existingSession,
        initialTurn,
        existingWorktreeRecord: existingArtifacts.worktreeRecord,
        worktree,
        sessionRecord,
      })
      await params.onPersisted?.({
        sessionId: id,
      })
      await events.emit("session.persisted", {
        sessionId: id,
        request: resolvedRequest,
      })
      publishSessionUpdated(id, ["status", "connection", "title", "contextUsage"])
      emitDiagnostic(
        id,
        "session_created",
        {
          status: initialized.status,
          ...sessionLogContext,
        },
        sessionLogger,
      )

      if (
        preparedTitle.titleState === "pending" &&
        preparedTitle.generatorConfig &&
        preparedTitle.promptText
      ) {
        sessionTitles.queueSessionTitleGeneration({
          id,
          generatorConfig: preparedTitle.generatorConfig,
          fallbackTitle: preparedTitle.title,
          promptText: preparedTitle.promptText,
          diagnosticLogger: sessionLogger,
        })
      }

      if (exitAfterInitialPrompt) {
        const completedSession = await completeOneShotLaunch({
          id,
          initialized,
          agentProcess,
          sessionLogger,
          supportsLoadSession: sessionSupportsLoad,
        })
        if (worktree) {
          await events.emit("session.launch.finished", {
            sessionId: id,
            reason: "one_shot_completed",
            worktree: toSessionWorktreeLifecycleState(worktree.state, id),
          })
        }
        return completedSession
      }

      const liveSession = await activateLiveSession({
        id,
        token,
        supportsLoadSession: sessionSupportsLoad,
        agentProcess,
        initialized,
        nextTurnSequence,
        sessionLogger,
        systemPrompt: resolvedRequest.systemPrompt,
        idleShutdownTimeoutMs: resolveIdleSessionShutdownTimeoutMs(resolvedConfig),
      })
      if (deferredInitialPrompt !== undefined) {
        void promptTurns.promptSession(id, deferredInitialPrompt).catch((error) => {
          emitDiagnostic(
            id,
            "session_initial_prompt_failed",
            {
              errorMessage: getErrorMessage(error),
            },
            sessionLogger,
          )
        })
      }
      if (worktree) {
        await events.emit("session.activated", {
          sessionId: id,
          worktree: toSessionWorktreeLifecycleState(worktree.state, id),
        })
      }
      return liveSession
    } catch (error) {
      sessionLogger.log("session.launch_failed", {
        sessionId: id,
        ...sessionLogContext,
        errorMessage: getErrorMessage(error),
      })
      if (spawnedAgentProcess && !activeSessions.has(id)) {
        await initializedClient?.close().catch(() => {})
        await treeKill(spawnedAgentProcess).catch(() => {})
        await waitForAgentProcessExit(spawnedAgentProcess).catch(() => {})
      }
      if (worktree) {
        await events.emit("session.launch.failed", {
          sessionId: id,
          error,
          worktree: toSessionWorktreeLifecycleState(worktree.state, id),
        })
      }
      if (!existingSession) {
        for (const turnRecord of db.sessionTurns.findMany({
          where: { sessionId: id },
        })) {
          await Promise.resolve(db.sessionTurns.delete(turnRecord.id)).catch(() => {})
        }
        const draftRecord =
          db.sessionTurnDrafts.first({
            where: { sessionId: id },
          }) ?? null
        if (draftRecord) {
          await Promise.resolve(db.sessionTurnDrafts.delete(draftRecord.id)).catch(() => {})
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
      throw createSessionIpcError(SessionErrorCodes.NotFound, { sessionId: params.id })
    }

    return launchSession(params, existingSession)
  }

  async function getSession(id: SessionId): Promise<DaemonSession> {
    await ready
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw createSessionIpcError(SessionErrorCodes.NotFound, {
        sessionId: id,
      })
    }
    return record
  }

  async function listSessions(params: ListSessionsRequest): Promise<ListSessionsResponse> {
    await ready
    const pageSize = normalizeSessionPageSize(params.limit)
    let page: ReturnType<typeof db.sessions.findPage>

    try {
      page = db.sessions.findPage({
        where: {
          completedHidden: false,
        },
        orderBy: {
          lastSessionActivityAt: "desc",
          id: "desc",
        },
        limit: pageSize,
        after: params.cursor ?? undefined,
      })
    } catch {
      throw createSessionIpcError(SessionErrorCodes.InvalidCursor, {
        cursor: params.cursor ?? null,
      })
    }

    return {
      sessions: page.items,
      nextCursor: page.next ?? null,
      hasMore: page.next != null,
    }
  }

  async function connectSession(id: SessionId): Promise<DaemonSession> {
    await ready
    const active = activeSessions.get(id)
    if (active) {
      emitDiagnostic(id, "session_connected", undefined, active.logger)
      return getSession(id)
    }

    const session = await getSession(id)
    if (session.connectionMode === "live" && session.supportsLoadSession && session.agent) {
      const reloadedSession = await loadSession({
        id,
        request: createReconnectRequest(session),
      })
      const reloadedActiveSession = activeSessions.get(id)
      emitDiagnostic(id, "session_connected", undefined, reloadedActiveSession?.logger ?? logger)
      return reloadedSession
    }

    throw createSessionIpcError(
      session.connectionMode === "history"
        ? SessionErrorCodes.ArchivedNotReconnectable
        : SessionErrorCodes.NotReconnectable,
      { connectionMode: session.connectionMode, sessionId: id },
    )
  }

  function readLatestTurnDraft(id: SessionId) {
    const draftRecord =
      db.sessionTurnDrafts.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      }) ?? null
    if (!draftRecord) {
      return null
    }

    const latestTurn =
      db.sessionTurns.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      }) ?? null
    if (latestTurn?.turnId === draftRecord.turnId) {
      db.sessionTurnDrafts.delete(draftRecord.id)
      return null
    }

    return draftRecord
  }

  async function getHistory(params: GetSessionHistoryRequest): Promise<GetSessionHistoryResponse> {
    await ready
    const session = await getSession(params.id)
    const pageSize = normalizeSessionPageSize(params.limit)
    let page: ReturnType<typeof db.sessionTurns.findPage>

    try {
      page = db.sessionTurns.findPage({
        where: { sessionId: params.id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
        limit: pageSize,
        after: params.cursor ?? undefined,
      })
    } catch {
      throw createSessionIpcError(SessionErrorCodes.InvalidHistoryCursor, {
        cursor: params.cursor ?? null,
        sessionId: params.id,
      })
    }

    const turns = [...page.items].reverse().map(toSessionHistoryTurnFromRecord)
    if (!params.cursor) {
      const active = activeSessions.get(params.id)

      if (active?.activeTurn) {
        turns.push(toSessionHistoryTurnFromActiveTurn(active.activeTurn))
      } else {
        const draftRecord = readLatestTurnDraft(params.id)
        if (draftRecord) {
          turns.push(toSessionHistoryTurnFromDraft(draftRecord))
        }
      }
    }

    emitDiagnostic(
      params.id,
      "session_history_read",
      {
        persistedTurnCount: page.items.length,
        returnedTurnCount: turns.length,
        hasCursor: params.cursor != null,
        hasMore: page.next != null,
      },
      logger,
      { logSessionId: false },
    )

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      connection: toConnectionState({
        mode: session.connectionMode,
        activeDaemonSession: session.activeDaemonSession,
      }),
      turns,
      nextCursor: page.next ?? null,
      hasMore: page.next != null,
    }
  }

  async function getChanges(id: SessionId): Promise<GetSessionChangesResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord = await sessionWorktrees.resolvePersistedWorktreeRecord(id)
    const changes = await readSessionChanges({
      cwd: session.cwd,
      worktree: worktreeRecord,
    })

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      workspaceRoot: changes.workspaceRoot,
      diff: changes.diff,
      hasChanges: changes.hasChanges,
    }
  }

  async function getComposerSuggestions(
    params: SessionComposerSuggestionsRequest,
  ): Promise<SessionComposerSuggestionsResponse> {
    await ready
    const session = await getSession(params.id)
    const limit = normalizeComposerSuggestionLimit(params.limit)

    if (params.trigger === "dollar") {
      return {
        suggestions: await getDraftComposerSuggestions({
          cwd: session.cwd,
          query: params.query,
          limit,
        }),
      }
    }

    return {
      suggestions: getSlashComposerSuggestions(session.availableCommands, params.query, limit),
    }
  }

  async function getDraftSuggestions(
    params: SessionDraftSuggestionsRequest,
  ): Promise<SessionComposerSuggestionsResponse> {
    await ready

    return {
      suggestions: await getDraftComposerSuggestions({
        cwd: params.cwd,
        query: params.query,
        limit: normalizeComposerSuggestionLimit(params.limit),
      }),
    }
  }

  async function getLaunchPreview(
    params: SessionLaunchPreviewRequest,
  ): Promise<SessionLaunchPreviewResponse> {
    await ready
    return launchPreparation.getLaunchPreview(params)
  }

  async function releaseLaunchLease(
    params: ReleaseSessionLaunchLeaseRequest,
  ): Promise<ReleaseSessionLaunchLeaseResponse> {
    await ready
    return {
      launchLeaseId: params.launchLeaseId,
      released: launchLeaseStore.scheduleReleaseById(params.launchLeaseId, "client_release"),
    }
  }

  async function prepareLaunchWorktree(
    params: PrepareSessionLaunchWorktreeRequest,
  ): Promise<PrepareSessionLaunchWorktreeResponse> {
    await ready
    return launchWorktrees.prepare(params)
  }

  async function releaseLaunchWorktree(
    params: ReleaseSessionLaunchWorktreeRequest,
  ): Promise<ReleaseSessionLaunchWorktreeResponse> {
    await ready
    return launchWorktrees.release(params)
  }

  async function getSubpackages(
    params: SessionSubpackagesRequest,
  ): Promise<SessionSubpackagesResponse> {
    await ready

    const config = await configProvider.getRootConfig(params.cwd).then((root) => root.config)

    return {
      subpackages: await discoverSessionSubpackages({
        cwd: params.cwd,
        configuredManifests: config?.subpackages?.manifests,
      }),
    }
  }

  async function getDiagnostics(id: SessionId): Promise<GetSessionDiagnosticsResponse> {
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
      events: (diagnosticsRecord?.events ?? []).map((event: DaemonSessionDiagnosticEvent) => ({
        ...event,
        sessionId: session.id,
      })),
    }
  }

  async function getWorktree(id: SessionId): Promise<GetSessionWorktreeResponse> {
    await ready
    return sessionWorktrees.getWorktree(id)
  }

  async function requireWorktree(id: SessionId): Promise<SessionWorktreeLifecycleState> {
    await ready
    return sessionWorktrees.requireWorktree(id)
  }

  async function listWorktrees(): Promise<SessionWorktreeLifecycleState[]> {
    await ready
    return sessionWorktrees.listWorktrees()
  }

  async function findWorktreeByDir(worktreeDir: string) {
    await ready
    return sessionWorktrees.findWorktreeByDir(worktreeDir)
  }

  function isActive(id: SessionId) {
    return activeSessions.has(id)
  }

  async function declareInitiative(id: SessionId, title: string) {
    await ready
    return sessionAttention.declareInitiative(id, title)
  }

  async function reportBlocker(
    id: SessionId,
    reason: string,
    metadata: AttentionMetadataInput = {},
  ) {
    await ready
    return sessionAttention.reportBlocker(id, reason, metadata)
  }

  async function reportTurnEnded(id: SessionId, metadata: AttentionMetadataInput = {}) {
    await ready
    return sessionAttention.reportTurnEnded(id, metadata)
  }

  async function recordTurnAttentionActivity(
    id: SessionId,
    metadata: AttentionMetadataInput & { fallbackHeadline?: string } = {},
  ) {
    await ready
    return sessionAttention.recordTurnAttentionActivity(id, metadata)
  }

  async function recordSessionResult(id: SessionId, message: string) {
    await ready
    await sessionAttention.recordSessionResult(id, message)
  }

  async function resolveTokenScope(token: string) {
    await ready
    const session =
      db.sessions.first({
        where: { token },
      }) ?? null
    if (!session?.permissions) {
      return null
    }

    return {
      sessionId: session.id,
      owner: session.permissions.owner,
      repo: session.permissions.repo,
      allowedPrNumbers: session.permissions.allowedPrNumbers,
    }
  }

  async function allowPullRequest(id: SessionId, prNumber: number) {
    await ready
    const session = requireSessionDocument(id)
    if (!session.permissions || session.permissions.allowedPrNumbers.includes(prNumber)) {
      return
    }

    updateSession(id, {
      permissions: {
        ...session.permissions,
        allowedPrNumbers: [...session.permissions.allowedPrNumbers, prNumber],
      },
    })
  }

  async function completeSession(id: SessionId) {
    await ready
    return sessionWorktrees.completeSession(id)
  }

  async function cancelSessionTurn(
    id: SessionId,
    options: {
      includePendingSteer?: boolean
      updateStatus: boolean
    } = { updateStatus: true },
  ): Promise<CancelSessionResponse> {
    await ready
    return promptTurns.cancelSessionTurn(id, options)
  }

  async function sendMessage(id: SessionId, message: acp.AnyMessage): Promise<void> {
    await ready
    await promptTurns.sendMessage(id, message)
  }

  async function setSessionConfigOption(params: SetSessionConfigOptionRequest) {
    await ready
    const active = activeSessions.get(params.id)
    if (!active) {
      throw createSessionIpcError(SessionErrorCodes.NotActive, { sessionId: params.id })
    }

    const result = await active.session.setConfigOption(params.configId, params.value)
    updateSession(params.id, {
      configOptions: result.configOptions,
    })
    return getSession(params.id)
  }

  async function setSessionModel(params: SetSessionModelRequest) {
    await ready
    const active = activeSessions.get(params.id)
    if (!active) {
      throw createSessionIpcError(SessionErrorCodes.NotActive, { sessionId: params.id })
    }

    const response = await active.session.setModel(params.modelId)
    updateSession(params.id, {
      configOptions: response.configOptions,
    })
    return getSession(params.id)
  }

  async function promptSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<acp.PromptResponse> {
    await ready
    return promptTurns.promptSession(id, prompt)
  }

  async function steerSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<SteerSessionResponse> {
    await ready
    return promptTurns.steerSession(id, prompt)
  }

  async function popQueuedPrompt(id: SessionId): Promise<PopQueuedSessionPromptResponse> {
    await ready
    return promptTurns.popQueuedPrompt(id)
  }

  async function shutdownSession(id: SessionId): Promise<boolean> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    idleShutdown.cancelIdleShutdownTimer(active, "session_shutdown")
    emitDiagnostic(id, "session_shutdown_requested", undefined, active.logger)
    const worktreeRecord = await sessionWorktrees.resolvePersistedWorktreeRecord(id)
    try {
      await events.emit("session.stopping", {
        sessionId: id,
        reason: "session_shutdown",
        worktree: worktreeRecord ? toSessionWorktreeLifecycleState(worktreeRecord, id) : null,
      })
    } catch (error) {
      emitDiagnostic(
        id,
        "session_shutdown_failed",
        {
          errorMessage: getErrorMessage(error),
        },
        active.logger,
      )
      return false
    }
    await treeKill(active.process)
    await waitForAgentProcessExit(active.process)
    await active.exitCleanup
    return true
  }

  async function resolveSessionIdByToken(token: string): Promise<SessionId> {
    await ready
    const record =
      db.sessions.first({
        where: { token },
      }) ?? null
    if (!record?.permissions) {
      throw createSessionIpcError(SessionErrorCodes.InvalidToken)
    }

    return record.id
  }

  async function close(): Promise<void> {
    await ready
    await launchLeaseStore.closeAll("daemon_shutdown")
    await launchWorktrees.close()

    for (const session of activeSessions.values()) {
      idleShutdown.cancelIdleShutdownTimer(session, "daemon_shutdown")
      const worktreeRecord = await sessionWorktrees.resolvePersistedWorktreeRecord(session.id)
      try {
        await events.emit("session.stopping", {
          sessionId: session.id,
          reason: "daemon_shutdown",
          worktree: worktreeRecord
            ? toSessionWorktreeLifecycleState(worktreeRecord, session.id)
            : null,
        })
      } catch {}
      emitDiagnostic(session.id, "daemon_shutdown", { status: session.status }, session.logger)
      await treeKill(session.process)
      await waitForAgentProcessExit(session.process)
      await session.exitCleanup
      await session.client.close().catch(() => {})
      const sessionRecord = db.sessions.get(session.id) ?? null
      if (sessionRecord?.permissions) {
        db.sessions.update(session.id, {
          token: null,
          permissions: null,
        })
      }
    }
    activeSessions.clear()
    activeSessionsByAcpSessionId.clear()
  }

  return {
    newSession,
    loadSession,
    listSessions,
    connectSession,
    getSession,
    getHistory,
    getChanges,
    getComposerSuggestions,
    getDraftSuggestions,
    getLaunchPreview,
    releaseLaunchLease,
    prepareLaunchWorktree,
    releaseLaunchWorktree,
    getSubpackages,
    getDiagnostics,
    getWorktree,
    requireWorktree,
    listWorktrees,
    findWorktreeByDir,
    isActive,
    emitDiagnostic,
    declareInitiative,
    reportBlocker,
    reportTurnEnded,
    recordTurnAttentionActivity,
    recordSessionResult,
    resolveTokenScope,
    allowPullRequest,
    completeSession,
    sendMessage,
    setSessionConfigOption,
    setSessionModel,
    cancelSessionTurn,
    steerSession,
    popQueuedPrompt,
    promptSession,
    shutdownSession,
    sessionSubscriberConnected,
    sessionSubscriberDisconnected,
    resolveSessionIdByToken,
    close,
  }
}
