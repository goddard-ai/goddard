import { randomBytes, randomUUID } from "node:crypto"
import { realpath } from "node:fs/promises"
import { resolve } from "node:path"
import treeKill from "@alloc/tree-kill"
import { resolveDefaultAgent } from "@goddard-ai/config"
import { IpcClientError } from "@goddard-ai/ipc"
import {
  listReviewSessions,
  startReviewSync,
  statusReviewSession,
  stopReviewSession,
  syncReviewSession,
  watchReviewSession,
  type ReviewSyncResult,
  type ReviewSyncStatusData,
} from "@goddard-ai/review-sync"
import type { UserConfig } from "@goddard-ai/schema/config"
import type {
  AbortedSessionPrompt,
  CancelSessionResponse,
  CreateSessionRequest,
  DaemonSession,
  DaemonSessionStatus,
  GetSessionChangesResponse,
  GetSessionDiagnosticsResponse,
  GetSessionHistoryRequest,
  GetSessionHistoryResponse,
  GetSessionWorkforceResponse,
  GetSessionWorktreeResponse,
  InboxHeadline,
  InboxScope,
  InitialPromptOption,
  ListSessionsRequest,
  ListSessionsResponse,
  MutateSessionReviewSessionResponse,
  ReleaseSessionLaunchLeaseRequest,
  ReleaseSessionLaunchLeaseResponse,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  SessionDraftSuggestionsRequest,
  SessionHistoryTurn,
  SessionInboxMetadataInput,
  SessionLaunchBranch,
  SessionLaunchPreviewRequest,
  SessionLaunchPreviewResponse,
  SessionSubpackagesRequest,
  SessionSubpackagesResponse,
  SteerSessionResponse,
} from "@goddard-ai/schema/daemon"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"
import {
  createAcpClient,
  createAgentConnection,
  getAcpMessageResult,
  isAcpRequest,
  matchAcpRequest,
  type AcpSession,
  type AgentInputStream,
  type AgentOutputStream,
} from "acp-client"
import * as acp from "acp-client/protocol"
import type { KindInput, KindOutput } from "kindstore"
import { getErrorMessage, omit } from "radashi"

import { loadDaemonTextModel } from "../ai/text-model-resolver.ts"
import type { ConfigManager } from "../config-manager.ts"
import { SessionContext } from "../context.ts"
import type { InboxManager } from "../inbox/manager.ts"
import { resolveInboxMetadata } from "../inbox/metadata.ts"
import { createChunkPreview, createLogger, createPayloadPreview } from "../logging.ts"
import {
  type SessionConnectionMode,
  type SessionDiagnosticEvent,
} from "../persistence/session-state.ts"
import { db } from "../persistence/store.ts"
import { prepareFreshWorktree } from "../worktrees/bootstrap.ts"
import { createWorktree } from "../worktrees/index.ts"
import { createWorktreePluginManager } from "../worktrees/plugin-manager.ts"
import { defaultPlugin } from "../worktrees/plugins/default.ts"
import {
  spawnAgentProcess,
  waitForAgentProcessExit,
  type AgentProcessHandle,
} from "./agent-process.ts"
import { readSessionChanges } from "./changes.ts"
import {
  getDraftComposerSuggestions,
  getSlashComposerSuggestions,
  MAX_COMPOSER_SUGGESTION_LIMIT,
  normalizeComposerSuggestionLimit,
} from "./composer-suggestions.ts"
import { createLaunchLeaseKey, createLaunchLeaseStore, type LaunchLease } from "./launch-lease.ts"
import type { ACPRegistryService } from "./registry.ts"
import {
  agentNameFromInput,
  createReconnectRequest,
  createSessionRecordUpdate,
  disconnectedConnectionMode,
  mergeSessionMetadata,
  parseRepoScope,
  persistLaunchedSession,
  resolveExistingSessionArtifacts,
  resolveLatestStoredTurnSequence,
  toConnectionState,
  type ResolvedCreateSessionRequest,
} from "./session-records.ts"
import { discoverSessionSubpackages } from "./subpackages.ts"
import { backfillSessionTitle, generateSessionTitle, prepareSessionTitle } from "./title.ts"
import {
  appendSessionHistoryMessage,
  createInitializedHistoryTurn,
  getAvailableCommandsFromMessage,
  getContextUsageFromMessage,
  getLatestAvailableCommands,
  getLatestContextUsage,
  isTurnTerminalMessage,
  shouldFlushTurnDraftImmediately,
  toCompletedTurnInput,
  toSessionHistoryTurnFromActiveTurn,
  toSessionHistoryTurnFromDraft,
  toSessionHistoryTurnFromRecord,
  toTurnDraftInput,
  type ActiveTurnBuffer,
  type SessionTurnPromptRequestId,
} from "./turn-history.ts"
import {
  inspectWorktreeCompletionState,
  resolveGitRepoRoot,
  reuseExistingWorktree,
  toPreparedSessionWorktree,
  type PreparedSessionWorktree,
  type SessionWorktreeState,
} from "./worktree.ts"

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

const logger = createLogger()

type SessionId = DaemonSession["id"]

const DEFAULT_IDLE_SESSION_SHUTDOWN_TIMEOUT_MS = 15 * 60 * 1000

/** Daemon session document shape used when reading sessions back from kindstore. */
type SessionDoc = KindOutput<typeof db.schema.sessions>
type SessionTurnDraftDoc = KindOutput<typeof db.schema.sessionTurnDrafts>
type SessionWorktreeDoc = KindOutput<typeof db.schema.worktrees>

type SessionTitleGeneratorConfig = NonNullable<
  NonNullable<UserConfig["sessionTitles"]>["generator"]
>

const QUEUED_PROMPT_ABORTED_ERROR_CODE = -32800
const QUEUED_PROMPT_ABORTED_ERROR_MESSAGE =
  "Queued prompt aborted before dispatch by session cancellation."

/** Lists local git branches for one launch dialog and keeps the current branch first. */
async function listLaunchBranches(cwd: string): Promise<SessionLaunchBranch[]> {
  const repoRoot = await resolveGitRepoRoot(cwd)

  if (!repoRoot) {
    return []
  }

  const result = Bun.spawn(
    ["git", "for-each-ref", "--format=%(if)%(HEAD)%(then)*%(end)%(refname:short)", "refs/heads"],
    {
      cwd: repoRoot,
      stdin: "ignore",
      stdout: "pipe",
      stderr: "ignore",
    },
  )
  const stdout = result.stdout ? await new Response(result.stdout).text() : ""
  await result.exited

  if (result.exitCode !== 0) {
    return []
  }

  const branches = stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => ({
      current: line.startsWith("*"),
      name: line.startsWith("*") ? line.slice(1) : line,
    }))

  branches.sort((left, right) => {
    if (left.current !== right.current) {
      return left.current ? -1 : 1
    }

    return left.name.localeCompare(right.name)
  })

  return branches
}

/** Returns true when branch switching in the requested local checkout would risk user work. */
async function inspectLaunchCheckoutDirty(cwd: string): Promise<boolean> {
  const repoRoot = await resolveGitRepoRoot(cwd)

  if (!repoRoot) {
    return false
  }

  const result = Bun.spawn(["git", "status", "--porcelain=v1", "--untracked-files=normal"], {
    cwd: repoRoot,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "ignore",
  })
  const stdout = result.stdout ? await new Response(result.stdout).text() : ""
  await result.exited

  return result.exitCode === 0 && stdout.trim().length > 0
}

/** Switches the user's local checkout before launching the first prompt. */
async function checkoutLocalBranch(params: { cwd: string; branchName: string }) {
  const repoRoot = await resolveGitRepoRoot(params.cwd)

  if (!repoRoot) {
    throw new IpcClientError("Cannot checkout a branch outside a git repository.")
  }

  if (await inspectLaunchCheckoutDirty(repoRoot)) {
    throw new IpcClientError("Cannot checkout a branch while the local checkout has changes.")
  }

  const result = Bun.spawn(["git", "checkout", params.branchName], {
    cwd: repoRoot,
    stdin: "ignore",
    stdout: "ignore",
    stderr: "pipe",
  })
  const stderr = result.stderr ? await new Response(result.stderr).text() : ""
  await result.exited

  if (result.exitCode !== 0) {
    throw new IpcClientError(
      `Cannot checkout branch ${params.branchName}: ${stderr.trim() || "git checkout failed"}`,
    )
  }
}

/** Applies launch-time ACP model and config-option choices before the first prompt runs. */
async function applyInitialSessionConfiguration(params: {
  session: AcpSession
  models: acp.SessionModelState | null | undefined
  configOptions: acp.SessionConfigOption[] | null | undefined
  request: CreateSessionRequest
}) {
  let models = params.models ?? null
  let configOptions = params.configOptions ?? []

  if (params.request.initialModelId) {
    await params.session.setModel(params.request.initialModelId)

    if (models) {
      models = {
        ...models,
        currentModelId: params.request.initialModelId,
      }
    }
  }

  for (const option of params.request.initialConfigOptions ?? []) {
    // ACP config option values are always string ids, even when the launch form captured
    // a boolean choice locally.
    const response = await params.session.setConfigOption(option.configId, String(option.value))

    configOptions = response.configOptions
  }

  return {
    models,
    configOptions,
  }
}

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

/** Narrows one agent notification to a structured session update payload. */
type SessionUpdateMessage = acp.AnyMessage & {
  params: acp.SessionNotification
}

/** Queue-backed prompt request owned by the daemon until it is sent or aborted. */
type QueuedPromptEntry = {
  requestId: string | number
  prompt: acp.ContentBlock[]
  source: "client" | "daemon"
  resolve?: (response: acp.PromptResponse) => void
  reject?: (error: Error) => void
}

/** Deferred steer request waiting for a safe boundary before dispatch. */
type PendingSteerRequest = {
  requestId: string
  cancelledRequestId: string | number
  prompt: acp.ContentBlock[]
  abortedQueue: AbortedSessionPrompt[]
  waitingForBoundary: boolean
  resolve: (response: SteerSessionResponse) => void
  reject: (error: Error) => void
}

/** Pending daemon-owned prompt request waiting for the agent response frame. */
type PendingPromptRequest = {
  resolve: (response: acp.PromptResponse) => void
  reject: (error: Error) => void
}

/** Holds the live runtime state for a daemon-owned session process. */
type ActiveSession = {
  id: SessionId
  acpSessionId: string
  logger: ReturnType<typeof createLogger>
  token: string
  supportsLoadSession: boolean
  process: AgentProcessHandle
  writer: WritableStreamDefaultWriter<acp.AnyMessage>
  subscription: {
    close: () => Promise<void>
  }
  status: DaemonSessionStatus
  nextTurnSequence: number
  activeTurn: ActiveTurnBuffer<SessionTurnDraftDoc["id"]> | null
  isFirstPrompt: boolean
  systemPrompt: string
  lastPermissionRequest: PermissionRequest | null
  clientRequests: ClientRequestMap
  pendingPrompts: Map<string | number, PendingPromptRequest>
  promptQueue: QueuedPromptEntry[]
  blockingPromptRequestId: string | number | null
  pendingSteer: PendingSteerRequest | null
  idleShutdownTimer: ReturnType<typeof setTimeout> | null
}

type ReviewSessionRuntime = {
  abortController: AbortController
  running: Promise<void>
}

/** Shared session-launch options resolved by the daemon before an agent process starts. */
interface SessionLaunchParams {
  request: CreateSessionRequest
  token?: string
  config?: UserConfig
  worktreePlugins?: WorktreePlugin[]
}

/** Fresh daemon session input accepted by `SessionManager.newSession()`. */
interface NewSessionParams extends SessionLaunchParams {}

/** Stored daemon session input accepted by `SessionManager.loadSession()`. */
interface LoadSessionParams extends SessionLaunchParams {
  id: SessionId
}

/** Exposes the daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = {
  newSession: (params: NewSessionParams) => Promise<DaemonSession>
  loadSession: (params: LoadSessionParams) => Promise<DaemonSession>
  listSessions: (params: ListSessionsRequest) => Promise<ListSessionsResponse>
  connectSession: (id: SessionId) => Promise<DaemonSession>
  getSession: (id: SessionId) => Promise<DaemonSession>
  getHistory: (params: GetSessionHistoryRequest) => Promise<GetSessionHistoryResponse>
  getChanges: (id: SessionId) => Promise<GetSessionChangesResponse>
  getComposerSuggestions: (
    params: SessionComposerSuggestionsRequest,
  ) => Promise<SessionComposerSuggestionsResponse>
  getDraftSuggestions: (
    params: SessionDraftSuggestionsRequest,
  ) => Promise<SessionComposerSuggestionsResponse>
  getLaunchPreview: (params: SessionLaunchPreviewRequest) => Promise<SessionLaunchPreviewResponse>
  releaseLaunchLease: (
    params: ReleaseSessionLaunchLeaseRequest,
  ) => Promise<ReleaseSessionLaunchLeaseResponse>
  getSubpackages: (params: SessionSubpackagesRequest) => Promise<SessionSubpackagesResponse>
  getDiagnostics: (id: SessionId) => Promise<GetSessionDiagnosticsResponse>
  getWorktree: (id: SessionId) => Promise<GetSessionWorktreeResponse>
  mountReviewSession: (id: SessionId) => Promise<MutateSessionReviewSessionResponse>
  runReviewSession: (id: SessionId) => Promise<MutateSessionReviewSessionResponse>
  unmountReviewSession: (id: SessionId) => Promise<MutateSessionReviewSessionResponse>
  getWorkforce: (id: SessionId) => Promise<GetSessionWorkforceResponse>
  declareInitiative: (id: SessionId, title: string) => Promise<DaemonSession>
  reportBlocker: (
    id: SessionId,
    reason: string,
    metadata?: SessionInboxMetadataInput,
  ) => Promise<DaemonSession>
  reportTurnEnded: (id: SessionId, metadata?: SessionInboxMetadataInput) => Promise<DaemonSession>
  recordTurnAttentionActivity: (
    id: SessionId,
    metadata?: SessionInboxMetadataInput & { fallbackHeadline?: string },
  ) => Promise<{ scope: InboxScope; headline: InboxHeadline; turnId: string | null }>
  completeSession: (id: SessionId) => Promise<ReturnType<InboxManager["completeSession"]>>
  sendMessage: (id: SessionId, message: acp.AnyMessage) => Promise<void>
  cancelSessionTurn: (id: SessionId) => Promise<CancelSessionResponse>
  steerSession: (
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ) => Promise<SteerSessionResponse>
  promptSession: (id: SessionId, prompt: string | acp.ContentBlock[]) => Promise<acp.PromptResponse>
  shutdownSession: (id: SessionId) => Promise<boolean>
  sessionSubscriberConnected: (id: SessionId) => Promise<void>
  sessionSubscriberDisconnected: (id: SessionId) => Promise<void>
  resolveSessionIdByToken: (token: string) => Promise<SessionId>
  close: () => Promise<void>
}

/** Ensures the daemon's system prompt is prepended to the first user prompt sent to an agent. */
export function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest {
  if (systemPrompt.length === 0) {
    return request
  }

  return {
    ...request,
    prompt: [
      {
        type: "text",
        text: `<system-prompt name="goddard">${systemPrompt}</system-prompt>`,
      },
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
  const promptRequest: acp.PromptRequest = {
    sessionId: params.sessionId,
    prompt:
      typeof params.prompt === "string" ? [{ type: "text", text: params.prompt }] : params.prompt,
  }

  return params.isFirstPrompt
    ? injectSystemPrompt(promptRequest, params.systemPrompt)
    : promptRequest
}

/** Maps client-originated ACP messages to any immediate session status changes they imply. */
function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: DaemonSessionStatus,
): DaemonSessionStatus | null {
  if (status !== "active") {
    return null
  }

  if (isAcpRequest(message, acp.AGENT_METHODS.session_cancel)) {
    return "cancelled"
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

/** Returns true when the ACP adapter can reopen this session later via `session/load`. */
function supportsSessionLoad(initialized: Pick<InitializedSession, "agentCapabilities">): boolean {
  return initialized.agentCapabilities?.loadSession === true
}

type InitializedSession = acp.InitializeResponse & {
  status: DaemonSessionStatus
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  initialPromptRequestId: SessionTurnPromptRequestId | null
  initialPromptStartedAt: string | null
  initialPromptCompletedAt: string | null
  acpSessionId: string
  models?: acp.SessionModelState | null
  configOptions?: acp.SessionConfigOption[] | null
  stopReason: acp.PromptResponse["stopReason"] | null
}

/** Runs an optional launch-time prompt and captures the synthetic turn history around it. */
async function runLaunchInitialPrompt(params: {
  session: AcpSession
  acpSessionId: string
  request: CreateSessionRequest
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
  request: CreateSessionRequest
  resumeAcpId?: string
  onMessageWrite?: (message: acp.AnyMessage) => void
}): Promise<InitializedSession> {
  const history: acp.AnyMessage[] = []
  const client = await createAcpClient({
    stdin: params.input,
    stdout: params.output,
    clientInfo: {
      name: "npm:@goddard-ai/daemon",
      version: getPackageVersion(),
    },
    handler: {
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params) {
        history.push({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params,
        })
      },
    },
  })

  try {
    const initializeResult = client.initialize

    let isFirstPrompt = true
    let acpSessionId: string
    let session: AcpSession
    let models: acp.SessionModelState | null | undefined
    let configOptions: acp.SessionConfigOption[] | null | undefined

    if (params.resumeAcpId !== undefined) {
      if (initializeResult.agentCapabilities?.loadSession !== true) {
        throw new IpcClientError(
          `Cannot resume ACP session ${params.resumeAcpId}: agent does not support session/load`,
        )
      }

      session = await client.loadSession({
        sessionId: params.resumeAcpId,
        cwd: params.request.cwd,
        mcpServers: params.request.mcpServers,
      })
      acpSessionId = params.resumeAcpId
      isFirstPrompt = false
    } else {
      session = await client.newSession(params.request)
      acpSessionId = session.sessionId
      models = session.models
      configOptions = session.configOptions

      if (
        params.request.initialModelId !== undefined ||
        (params.request.initialConfigOptions?.length ?? 0) > 0
      ) {
        const configuredSession = await applyInitialSessionConfiguration({
          session,
          models,
          configOptions,
          request: params.request,
        })

        models = configuredSession.models
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
      history,
      acpSessionId,
      models,
      configOptions,
    }
  } finally {
    await client.close()
  }
}

/** Promotes one prepared launch lease by applying final launch options and optional initial prompt. */
async function initializeSessionFromLaunchLease(params: {
  lease: LaunchLease
  request: CreateSessionRequest
  onMessageWrite?: (message: acp.AnyMessage) => void
}) {
  try {
    let models: acp.SessionModelState | null | undefined = params.lease.models
    let configOptions: acp.SessionConfigOption[] | null | undefined = params.lease.configOptions

    if (
      params.request.initialModelId !== undefined ||
      (params.request.initialConfigOptions?.length ?? 0) > 0
    ) {
      const configuredSession = await applyInitialSessionConfiguration({
        session: params.lease.session,
        models,
        configOptions,
        request: params.request,
      })

      models = configuredSession.models
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
      history: params.lease.history,
      acpSessionId: params.lease.acpSessionId,
      models,
      configOptions,
    } satisfies InitializedSession
  } finally {
    await params.lease.client.close().catch(() => {})
  }
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
  sessionId: SessionId
  request: CreateSessionRequest
  existingWorktree: SessionWorktreeState | null
  worktreePlugins?: WorktreePlugin[]
  defaultWorktreesFolder?: string
}) {
  if (params.existingWorktree) {
    await reuseExistingWorktree(params.existingWorktree, {
      worktreePlugins: params.worktreePlugins,
    })
    return toPreparedSessionWorktree(params.existingWorktree)
  }

  if (params.request.worktree?.enabled !== true) {
    return null
  }

  const repoRoot = await resolveGitRepoRoot(params.request.cwd)
  if (!repoRoot) {
    return null
  }

  return toPreparedSessionWorktree(
    await createWorktree({
      cwd: repoRoot,
      requestedCwd: params.request.cwd,
      branchName:
        typeof params.request.prNumber === "number"
          ? `pr-${params.request.prNumber}`
          : `goddard-${params.sessionId}`,
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
  workforce?: CreateSessionRequest["workforce"]
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

/** Builds the stable async context installed while one daemon session is actively doing work. */
function buildSessionContext(params: {
  sessionId: SessionId
  request: ResolvedCreateSessionRequest
  cwd: string
  worktree?: PreparedSessionWorktree | null
}) {
  const sessionContext: SessionContext = {
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
  config?: UserConfig,
): Promise<ResolvedCreateSessionRequest> {
  return {
    ...request,
    agent: request.agent ?? (await resolveDefaultAgent(config)),
  }
}

/** Logs ACP messages in a structured form without dumping full payloads verbatim. */
function logAgentMessage(
  diagnosticLogger: ReturnType<typeof createLogger>,
  event: "agent.message_read" | "agent.message_write",
  sessionId: SessionId,
  acpSessionId: string | undefined,
  message: acp.AnyMessage,
): void {
  diagnosticLogger.log(event, {
    sessionId,
    acpSessionId,
    direction: event === "agent.message_read" ? "read" : "write",
    hasId: "id" in message && message.id != null,
    method: "method" in message ? message.method : undefined,
    message: createPayloadPreview(message),
  })
}

/** Normalizes one queued prompt back into the client-facing aborted-queue payload. */
function toAbortedQueuedPrompt(entry: {
  requestId: string | number
  prompt: acp.ContentBlock[]
}): AbortedSessionPrompt {
  return {
    requestId: entry.requestId,
    prompt: [...entry.prompt],
  }
}

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
  for (const queued of active.promptQueue) {
    queued.reject?.(error)
  }
  active.promptQueue.length = 0
  if (active.pendingSteer) {
    active.pendingSteer.reject(error)
    active.pendingSteer = null
  }
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
  publish: (id: SessionId, message: acp.AnyMessage) => void
  inboxManager: InboxManager
  configManager: ConfigManager
  registryService: ACPRegistryService
  idleSessionShutdownTimeoutMs?: number
}): SessionManager {
  const activeSessions = new Map<SessionId, ActiveSession>()
  const launchLeaseStore = createLaunchLeaseStore({ logger })
  const sessionSubscriberCounts = new Map<SessionId, number>()
  const pendingSessionTitlePreparations = new Map<SessionId, Promise<void>>()
  const pendingSessionTitleGenerations = new Map<SessionId, Promise<void>>()
  const reviewSessionRuntimes = new Map<SessionId, ReviewSessionRuntime>()
  const idleSessionShutdownTimeoutMs =
    input.idleSessionShutdownTimeoutMs ?? DEFAULT_IDLE_SESSION_SHUTDOWN_TIMEOUT_MS
  const worktreePluginManager = createWorktreePluginManager({
    configManager: input.configManager,
    logger,
  })
  const ready = reconcilePersistedSessions()

  function updateSession(
    id: SessionId,
    update: Partial<KindInput<typeof db.schema.sessions>>,
    detail?: Record<string, unknown>,
    diagnosticLogger?: ReturnType<typeof createLogger>,
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

  function requireSessionDocument(id: SessionId) {
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw new IpcClientError(`Unknown session: ${id}`)
    }

    return record
  }

  function resolveCurrentTurnId(id: SessionId) {
    const activeTurn = activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      return activeTurn.turnId
    }

    return (
      db.sessionTurns.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      })?.turnId ?? null
    )
  }

  function applyInboxMetadataToCurrentTurn(
    id: SessionId,
    metadata: { scope: InboxScope; headline: InboxHeadline },
  ) {
    const activeTurn = activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      activeTurn.inboxScope = metadata.scope
      activeTurn.inboxHeadline = metadata.headline
      return
    }

    const latestTurn =
      db.sessionTurns.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      }) ?? null
    if (latestTurn) {
      db.sessionTurns.update(latestTurn.id, {
        inboxScope: metadata.scope,
        inboxHeadline: metadata.headline,
      })
    }
  }

  function resolveAndPersistInboxMetadata(input: {
    session: SessionDoc
    metadata?: SessionInboxMetadataInput & { fallbackHeadline?: string }
    blockedReason?: string | null
  }) {
    const resolved = resolveInboxMetadata({
      session: {
        ...input.session,
        blockedReason: input.blockedReason ?? input.session.blockedReason,
      },
      scope: input.metadata?.scope,
      headline: input.metadata?.headline,
      fallbackHeadline: input.metadata?.fallbackHeadline,
    })
    updateSession(input.session.id, { inboxScope: resolved.scope })
    applyInboxMetadataToCurrentTurn(input.session.id, resolved)
    return resolved
  }

  function clearTurnDraftFlushTimer(activeTurn: ActiveTurnBuffer | null) {
    if (!activeTurn?.flushTimer) {
      return
    }

    clearTimeout(activeTurn.flushTimer)
    activeTurn.flushTimer = null
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
    return true
  }

  function flushActiveTurnDraft(active: ActiveSession, reason: string) {
    const activeTurn = active.activeTurn
    if (!activeTurn) {
      return
    }

    clearTurnDraftFlushTimer(activeTurn)
    const existingDraft =
      (activeTurn.draftId && db.sessionTurnDrafts.get(activeTurn.draftId)) ||
      db.sessionTurnDrafts.first({
        where: { sessionId: active.id },
      }) ||
      null
    const draftInput = toTurnDraftInput(active.id, activeTurn)

    if (existingDraft) {
      activeTurn.draftId = existingDraft.id
      db.sessionTurnDrafts.put(existingDraft.id, draftInput)
    } else {
      activeTurn.draftId = db.sessionTurnDrafts.create(draftInput).id
    }

    emitDiagnostic(
      active.id,
      "session_turn_draft_flushed",
      {
        reason,
        turnId: activeTurn.turnId,
        sequence: activeTurn.sequence,
        messageCount: activeTurn.messages.length,
      },
      active.logger,
    )
  }

  function scheduleActiveTurnDraftFlush(active: ActiveSession, reason: string, immediate = false) {
    const activeTurn = active.activeTurn
    if (!activeTurn) {
      return
    }

    if (immediate) {
      flushActiveTurnDraft(active, reason)
      return
    }

    clearTurnDraftFlushTimer(activeTurn)
    activeTurn.flushTimer = setTimeout(() => {
      try {
        flushActiveTurnDraft(active, reason)
      } catch {}
    }, 100)
  }

  function persistTurnDraftAsInterruptedTurn(
    sessionId: SessionId,
    draftRecord: SessionTurnDraftDoc,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    const existingTurn =
      db.sessionTurns.first({
        where: { sessionId, sequence: draftRecord.sequence },
      }) ?? null

    if (existingTurn?.turnId === draftRecord.turnId) {
      db.sessionTurnDrafts.delete(draftRecord.id)
      return existingTurn
    }

    const turn = toSessionHistoryTurnFromDraft(draftRecord)
    const createdTurn = existingTurn
      ? db.sessionTurns.put(existingTurn.id, toCompletedTurnInput(sessionId, turn))
      : db.sessionTurns.create(toCompletedTurnInput(sessionId, turn))

    db.sessionTurnDrafts.delete(draftRecord.id)
    emitDiagnostic(
      sessionId,
      "session_turn_draft_promoted",
      {
        turnId: draftRecord.turnId,
        sequence: draftRecord.sequence,
      },
      diagnosticLogger,
    )
    return createdTurn
  }

  function appendTurnScopedMessage(active: ActiveSession, message: acp.AnyMessage) {
    const availableCommands = getAvailableCommandsFromMessage(message)
    if (availableCommands) {
      updateSessionAvailableCommands(active.id, availableCommands)
    }

    if (updateSessionContextUsage(active.id, message)) {
      return
    }

    const activeTurn = active.activeTurn
    if (!activeTurn) {
      return
    }

    appendSessionHistoryMessage(activeTurn.messages, message)
    scheduleActiveTurnDraftFlush(
      active,
      shouldFlushTurnDraftImmediately(activeTurn, message) ? "boundary" : "stream",
      shouldFlushTurnDraftImmediately(activeTurn, message),
    )
  }

  function finalizeActiveTurn(active: ActiveSession, message: acp.AnyMessage) {
    const activeTurn = active.activeTurn
    if (!activeTurn || !isTurnTerminalMessage(activeTurn, message)) {
      return
    }

    const completionKind = "error" in message ? "error" : "result"
    const stopReason =
      completionKind === "result"
        ? (getAcpMessageResult<acp.PromptResponse>(message)?.stopReason ?? null)
        : null
    const completedTurn: SessionHistoryTurn = {
      turnId: activeTurn.turnId,
      sequence: activeTurn.sequence,
      promptRequestId: activeTurn.promptRequestId,
      startedAt: activeTurn.startedAt,
      completedAt: new Date().toISOString(),
      completionKind,
      stopReason,
      inboxScope: activeTurn.inboxScope ?? null,
      inboxHeadline: activeTurn.inboxHeadline ?? null,
      messages: [...activeTurn.messages],
    }

    flushActiveTurnDraft(active, "completion")
    db.batch(() => {
      db.sessionTurns.create(toCompletedTurnInput(active.id, completedTurn))
      if (activeTurn.draftId) {
        db.sessionTurnDrafts.delete(activeTurn.draftId)
      } else {
        const draftRecord =
          db.sessionTurnDrafts.first({
            where: { sessionId: active.id },
          }) ?? null
        if (draftRecord) {
          db.sessionTurnDrafts.delete(draftRecord.id)
        }
      }
    })
    clearTurnDraftFlushTimer(activeTurn)
    active.activeTurn = null
    active.nextTurnSequence = Math.max(active.nextTurnSequence, completedTurn.sequence + 1)
    refreshIdleShutdownState(active.id, "turn_completed")
    emitDiagnostic(
      active.id,
      "session_turn_persisted",
      {
        turnId: completedTurn.turnId,
        sequence: completedTurn.sequence,
        completionKind: completedTurn.completionKind,
        stopReason: completedTurn.stopReason ?? undefined,
        messageCount: completedTurn.messages.length,
      },
      active.logger,
    )
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
    diagnosticLogger: ReturnType<typeof createLogger> = logger,
  ) {
    const event: SessionDiagnosticEvent = {
      type,
      at: new Date().toISOString(),
      detail,
    }
    diagnosticLogger.log(type, { sessionId, ...detail })
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

  /** Starts one detached title-generation task for a session whose fallback title is already persisted. */
  function queueSessionTitleGeneration(params: {
    id: SessionId
    generatorConfig: SessionTitleGeneratorConfig
    fallbackTitle: string
    promptText: string
    diagnosticLogger?: ReturnType<typeof createLogger>
  }) {
    if (pendingSessionTitleGenerations.has(params.id)) {
      return
    }

    const task = (async () => {
      const sessionRecord = db.sessions.get(params.id) ?? null
      if (!sessionRecord || sessionRecord.titleState !== "pending") {
        return
      }

      emitDiagnostic(
        params.id,
        "session_title_generation_started",
        {
          provider: params.generatorConfig.provider,
          model: params.generatorConfig.model,
        },
        params.diagnosticLogger,
      )

      try {
        const loadedTextModel = await loadDaemonTextModel(params.generatorConfig)
        const generatedTitle = await generateSessionTitle({
          model: loadedTextModel.model,
          promptText: params.promptText,
        })
        if (!generatedTitle) {
          throw new Error("Generated session title was empty or invalid.")
        }

        updateSession(
          params.id,
          {
            title: generatedTitle,
            titleState: "generated",
          },
          undefined,
          params.diagnosticLogger,
        )
        emitDiagnostic(
          params.id,
          "session_title_generated",
          {
            provider: loadedTextModel.descriptor.provider,
            model: loadedTextModel.descriptor.model,
            title: generatedTitle,
          },
          params.diagnosticLogger,
        )
      } catch (error) {
        updateSession(
          params.id,
          {
            title: params.fallbackTitle,
            titleState: "failed",
          },
          undefined,
          params.diagnosticLogger,
        )
        emitDiagnostic(
          params.id,
          "session_title_generation_failed",
          {
            provider: params.generatorConfig.provider,
            model: params.generatorConfig.model,
            errorMessage: getErrorMessage(error),
          },
          params.diagnosticLogger,
        )
      }
    })().finally(() => {
      pendingSessionTitleGenerations.delete(params.id)
    })

    pendingSessionTitleGenerations.set(params.id, task)
  }

  /** Initializes the first prompt-derived title for placeholder sessions without blocking prompt flow. */
  function queueSessionTitlePreparation(params: {
    id: SessionId
    prompt: string | acp.ContentBlock[]
    diagnosticLogger?: ReturnType<typeof createLogger>
  }) {
    const sessionRecord = db.sessions.get(params.id) ?? null
    if (
      !sessionRecord ||
      sessionRecord.titleState !== "placeholder" ||
      pendingSessionTitlePreparations.has(params.id)
    ) {
      return
    }

    const task = (async () => {
      let generatorConfig = input.configManager.getLastKnownRootConfig(sessionRecord.cwd)?.config
        .sessionTitles?.generator

      if (!generatorConfig) {
        try {
          generatorConfig = (await input.configManager.getRootConfig(sessionRecord.cwd)).config
            .sessionTitles?.generator
        } catch {}
      }

      const preparedTitle = prepareSessionTitle(params.prompt, generatorConfig)
      if (preparedTitle.titleState === "placeholder" || !preparedTitle.promptText) {
        return
      }

      updateSession(
        params.id,
        {
          title: preparedTitle.title,
          titleState: preparedTitle.titleState,
        },
        undefined,
        params.diagnosticLogger,
      )

      if (preparedTitle.titleState === "pending" && preparedTitle.generatorConfig) {
        queueSessionTitleGeneration({
          id: params.id,
          generatorConfig: preparedTitle.generatorConfig,
          fallbackTitle: preparedTitle.title,
          promptText: preparedTitle.promptText,
          diagnosticLogger: params.diagnosticLogger,
        })
      }
    })()
      .catch((error) => {
        emitDiagnostic(
          params.id,
          "session_title_generation_failed",
          {
            errorMessage: getErrorMessage(error),
          },
          params.diagnosticLogger,
        )
      })
      .finally(() => {
        pendingSessionTitlePreparations.delete(params.id)
      })

    pendingSessionTitlePreparations.set(params.id, task)
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
  }

  function toSessionWorktreeValue(
    record: SessionWorktreeDoc,
    reviewSession: ReviewSyncStatusData | null,
  ) {
    const { id: _id, sessionId: _sessionId, ...worktree } = record
    return {
      ...worktree,
      reviewSession,
    }
  }

  async function resolvePersistedWorktreeRecord(id: SessionId) {
    return (
      db.worktrees.first({
        where: { sessionId: id },
      }) ?? null
    )
  }

  async function resolveReviewSessionState(worktreeRecord: SessionWorktreeDoc | null) {
    if (!worktreeRecord) {
      return null
    }

    return await readReviewSessionState(worktreeRecord)
  }

  async function readReviewSessionState(worktreeRecord: SessionWorktreeDoc | SessionWorktreeState) {
    try {
      const result = await statusReviewSession({
        cwd: worktreeRecord.worktreeDir,
        json: true,
      })
      return result.data ?? null
    } catch (error) {
      if (isMissingReviewSessionError(error)) {
        return null
      }
      throw error
    }
  }

  async function readRequiredReviewSessionState(
    worktreeRecord: SessionWorktreeDoc | SessionWorktreeState,
  ) {
    const state = await readReviewSessionState(worktreeRecord)
    if (!state) {
      throw new Error(`Review session is missing for ${worktreeRecord.worktreeDir}.`)
    }
    return state
  }

  async function findMountedReviewSessionByPrimaryDir(primaryDir: string) {
    const normalizedPrimaryDir = await normalizeExistingPath(primaryDir)
    const sessions = await listReviewSessions({ cwd: normalizedPrimaryDir })
    return sessions.find((session) => session.reviewWorktree === normalizedPrimaryDir) ?? null
  }

  function findPersistedWorktreeRecordByDir(worktreeDir: string) {
    return db.worktrees.findMany().find((record) => record.worktreeDir === worktreeDir) ?? null
  }

  function createReviewSessionWarnings(result: ReviewSyncResult) {
    return result.status === "rejected-human-patch" ? [result.message] : []
  }

  function emitReviewSessionWarnings(
    id: SessionId,
    reason: string,
    result: ReviewSyncResult,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    for (const warning of createReviewSessionWarnings(result)) {
      emitDiagnostic(
        id,
        "review_session.warning",
        {
          reason,
          warning,
          acceptedPatchPath: result.acceptedPatchPath,
          rejectedPatchPath: result.rejectedPatchPath,
        },
        diagnosticLogger,
      )
    }
  }

  function emitReviewSessionResult(
    id: SessionId,
    reason: string,
    result: ReviewSyncResult,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    if (result.status === "error") {
      emitDiagnostic(
        id,
        "review_session.warning",
        { reason, errorMessage: result.message },
        diagnosticLogger,
      )
      return
    }
    emitReviewSessionWarnings(id, reason, result, diagnosticLogger)
  }

  function isMissingReviewSessionError(error: unknown) {
    return (
      error instanceof Error &&
      error.message.includes("No review-sync session matches the current worktree.")
    )
  }

  async function normalizeExistingPath(value: string) {
    return await realpath(resolve(value))
  }

  async function stopReviewSessionRuntime(id: SessionId) {
    const runtime = reviewSessionRuntimes.get(id)
    if (!runtime) {
      return
    }

    runtime.abortController.abort()
    reviewSessionRuntimes.delete(id)
    await runtime.running.catch(() => {})
  }

  /** Returns how many `session.message` stream subscribers are currently attached to one session id. */
  function getSessionSubscriberCount(id: SessionId): number {
    return sessionSubscriberCounts.get(id) ?? 0
  }

  /** Checks whether one live session is quiescent enough for idle auto-shutdown. */
  function shouldStartIdleShutdownTimer(active: ActiveSession): boolean {
    return (
      active.supportsLoadSession &&
      getSessionSubscriberCount(active.id) === 0 &&
      active.activeTurn === null &&
      active.blockingPromptRequestId === null &&
      active.promptQueue.length === 0 &&
      active.pendingSteer === null &&
      active.lastPermissionRequest === null
    )
  }

  /** Cancels one pending idle auto-shutdown timer and records the reason for that cancellation. */
  function cancelIdleShutdownTimer(active: ActiveSession, reason: string) {
    if (!active.idleShutdownTimer) {
      return
    }

    clearTimeout(active.idleShutdownTimer)
    active.idleShutdownTimer = null
    emitDiagnostic(
      active.id,
      "session_idle_shutdown_timer_cancelled",
      { reason, timeoutMs: idleSessionShutdownTimeoutMs },
      active.logger,
    )
  }

  /** Re-checks whether one active session should have an idle auto-shutdown timer armed right now. */
  function refreshIdleShutdownState(id: SessionId, reason: string) {
    const active = activeSessions.get(id)
    if (!active) {
      return
    }

    if (!shouldStartIdleShutdownTimer(active)) {
      cancelIdleShutdownTimer(active, reason)
      return
    }

    if (active.idleShutdownTimer) {
      return
    }

    emitDiagnostic(
      active.id,
      "session_idle_shutdown_timer_started",
      { reason, timeoutMs: idleSessionShutdownTimeoutMs },
      active.logger,
    )
    active.idleShutdownTimer = setTimeout(() => {
      void handleIdleShutdownTimerExpired(active.id).catch((error) => {
        logger.log("session_idle_shutdown_timer_failed", {
          sessionId: active.id,
          errorMessage: getErrorMessage(error),
        })
      })
    }, idleSessionShutdownTimeoutMs)
  }

  /** Shuts down one loadable idle session when its auto-shutdown timer expires without any reconnect. */
  async function handleIdleShutdownTimerExpired(id: SessionId): Promise<void> {
    const active = activeSessions.get(id)
    if (!active) {
      return
    }

    active.idleShutdownTimer = null
    if (!shouldStartIdleShutdownTimer(active)) {
      return
    }

    emitDiagnostic(
      id,
      "session_idle_shutdown_timer_expired",
      { timeoutMs: idleSessionShutdownTimeoutMs },
      active.logger,
    )
    await shutdownSession(id)
  }

  /** Records one new `session.message` subscriber so idle shutdown waits for every attached client to leave. */
  async function sessionSubscriberConnected(id: SessionId): Promise<void> {
    await ready
    sessionSubscriberCounts.set(id, getSessionSubscriberCount(id) + 1)
    refreshIdleShutdownState(id, "subscriber_connected")
  }

  /** Records one departing `session.message` subscriber and starts the timer when the last one leaves. */
  async function sessionSubscriberDisconnected(id: SessionId): Promise<void> {
    await ready
    const current = getSessionSubscriberCount(id)
    if (current <= 1) {
      sessionSubscriberCounts.delete(id)
    } else {
      sessionSubscriberCounts.set(id, current - 1)
    }
    refreshIdleShutdownState(id, "subscriber_disconnected")
  }

  async function runReviewSessionCycle(
    id: SessionId,
    worktreeRecord: SessionWorktreeDoc | SessionWorktreeState,
    reason: string,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    emitDiagnostic(id, "review_session.started", { reason }, diagnosticLogger)
    const result = await syncReviewSession({ cwd: worktreeRecord.worktreeDir })
    emitReviewSessionWarnings(id, reason, result, diagnosticLogger)
    const state = await readRequiredReviewSessionState(worktreeRecord)
    emitDiagnostic(
      id,
      "review_session.completed",
      {
        reason,
        warningCount: createReviewSessionWarnings(result).length,
        lastSync: state.lastSync,
      },
      diagnosticLogger,
    )
    return {
      state,
      warnings: createReviewSessionWarnings(result),
    }
  }

  async function startReviewSessionRuntime(
    id: SessionId,
    worktreeRecord: SessionWorktreeDoc | SessionWorktreeState,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    await stopReviewSessionRuntime(id)

    const state = await readReviewSessionState(worktreeRecord)
    if (!state || !activeSessions.has(id)) {
      return
    }

    const abortController = new AbortController()
    const running = watchReviewSession({
      cwd: worktreeRecord.worktreeDir,
      agentBranch: worktreeRecord.branchName,
      signal: abortController.signal,
      onResult: (result) => {
        emitReviewSessionResult(id, "watch", result, diagnosticLogger)
      },
    })
      .then((result) => {
        emitReviewSessionResult(id, "watch", result, diagnosticLogger)
      })
      .catch((error) => {
        if (abortController.signal.aborted) {
          return
        }
        emitDiagnostic(
          id,
          "review_session.warning",
          {
            reason: "watch",
            errorMessage: getErrorMessage(error),
          },
          diagnosticLogger,
        )
      })

    reviewSessionRuntimes.set(id, { abortController, running })
    emitDiagnostic(
      id,
      "review_session.watcher_started",
      { agentBranch: state.agentBranch, reviewBranch: state.reviewBranch },
      diagnosticLogger,
    )
  }

  async function replaceMountedReviewSessionIfNeeded(
    id: SessionId,
    worktreeRecord: SessionWorktreeDoc | SessionWorktreeState,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    const mounted = await findMountedReviewSessionByPrimaryDir(worktreeRecord.repoRoot)
    const previousWorktreeRecord = mounted
      ? findPersistedWorktreeRecordByDir(mounted.agentWorktree)
      : null
    if (!mounted || previousWorktreeRecord?.sessionId === id) {
      return
    }

    if (previousWorktreeRecord) {
      await stopReviewSessionRuntime(previousWorktreeRecord.sessionId)
    }
    await stopReviewSession({ cwd: mounted.agentWorktree })

    if (previousWorktreeRecord) {
      emitDiagnostic(
        previousWorktreeRecord.sessionId,
        "review_session.replaced",
        { replacedBySessionId: id },
        activeSessions.get(previousWorktreeRecord.sessionId)?.logger ?? logger,
      )
    }

    emitDiagnostic(
      id,
      "review_session.replaced",
      {
        previousSessionId: previousWorktreeRecord?.sessionId ?? null,
        previousReviewSessionId: mounted.sessionId,
      },
      diagnosticLogger,
    )
  }

  async function mountReviewSessionForWorktree(
    id: SessionId,
    worktreeRecord: SessionWorktreeDoc | SessionWorktreeState,
    diagnosticLogger: ReturnType<typeof createLogger>,
  ) {
    await replaceMountedReviewSessionIfNeeded(id, worktreeRecord, diagnosticLogger)
    const result = await startReviewSync({
      cwd: worktreeRecord.repoRoot,
      agentBranch: worktreeRecord.branchName,
    })
    emitReviewSessionResult(id, "mount", result, diagnosticLogger)
    const state = await readRequiredReviewSessionState(worktreeRecord)
    emitDiagnostic(
      id,
      "review_session.mounted",
      {
        reviewSessionId: state.sessionId,
        agentBranch: state.agentBranch,
        reviewBranch: state.reviewBranch,
      },
      diagnosticLogger,
    )
    return state
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

        const worktreeRecord = await resolvePersistedWorktreeRecord(session.id)
        if (worktreeRecord) {
          const mountedReviewSessionState = await readReviewSessionState(worktreeRecord).catch(
            () => null,
          )
          if (mountedReviewSessionState) {
            try {
              const stopped = await stopReviewSession({ cwd: worktreeRecord.worktreeDir })
              emitDiagnostic(session.id, "review_session.unmounted", {
                reason: "daemon_reconciliation",
                reviewSessionId: stopped.sessionId,
              })
            } catch (error) {
              emitDiagnostic(session.id, "review_session.warning", {
                reason: "daemon_reconciliation",
                errorMessage: getErrorMessage(error),
              })
            }
          }
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
              .sort((left, right) => left.sequence - right.sequence)
              .flatMap((turn) => turn.messages),
            ...(draftRecord?.messages ?? []),
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
          persistTurnDraftAsInterruptedTurn(session.id, draftRecord, logger)
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

  function normalizePrompt(prompt: string | acp.ContentBlock[]): acp.ContentBlock[] {
    return typeof prompt === "string" ? [{ type: "text", text: prompt }] : [...prompt]
  }

  function publishSessionMessage(
    active: ActiveSession,
    message: acp.AnyMessage,
    options: {
      persistTurnMessage?: boolean
    } = {},
  ) {
    if (options.persistTurnMessage !== false) {
      appendTurnScopedMessage(active, message)
    }
    input.publish(active.id, message)
  }

  async function writeImmediateMessage(
    active: ActiveSession,
    message: acp.AnyMessage,
    options: {
      updateStatus?: boolean
      persistTurnMessage?: boolean
      onBeforePublish?: (message: acp.AnyMessage) => Promise<void> | void
    } = {},
  ): Promise<void> {
    let clearedPermissionRequest = false
    if (
      active.lastPermissionRequest &&
      "id" in message &&
      message.id === active.lastPermissionRequest.id
    ) {
      active.lastPermissionRequest = null
      clearedPermissionRequest = true
    } else if (options.updateStatus !== false) {
      const nextStatus = sessionStatusFromClientMessage(message, active.status)
      if (nextStatus) {
        updateSession(
          active.id,
          { status: nextStatus },
          {
            reason: "client_message",
            method: "method" in message ? message.method : undefined,
            messageId: "id" in message ? message.id : undefined,
          },
        )
      }
    }

    if (
      active.isFirstPrompt &&
      isAcpRequest<PromptRequestMessage>(message, acp.AGENT_METHODS.session_prompt)
    ) {
      active.isFirstPrompt = false
      message.params = injectSystemPrompt(message.params, active.systemPrompt)
    }

    if ("id" in message && message.id != null && "method" in message) {
      active.clientRequests.set(message.id, message as acp.AnyMessage & { method: string })
    }

    if (clearedPermissionRequest) {
      refreshIdleShutdownState(active.id, "permission_request_resolved")
    }

    logAgentMessage(active.logger, "agent.message_write", active.id, active.acpSessionId, message)
    emitDiagnostic(
      active.id,
      "session_message_sent",
      {
        hasId: "id" in message && message.id != null,
        method: "method" in message ? message.method : undefined,
      },
      active.logger,
    )
    await options.onBeforePublish?.(message)
    publishSessionMessage(active, message, {
      persistTurnMessage: options.persistTurnMessage,
    })
    await active.writer.write(message)
  }

  async function processPromptQueue(active: ActiveSession): Promise<void> {
    if (active.blockingPromptRequestId !== null || active.pendingSteer?.waitingForBoundary) {
      return
    }

    const nextPrompt = active.promptQueue.shift()
    if (!nextPrompt) {
      return
    }

    const message = {
      jsonrpc: "2.0",
      id: nextPrompt.requestId,
      method: acp.AGENT_METHODS.session_prompt,
      params: {
        sessionId: active.acpSessionId,
        prompt: [...nextPrompt.prompt],
      },
    } satisfies acp.AnyMessage & {
      id: string | number
      method: string
      params: acp.PromptRequest
    }
    const existingDraft =
      db.sessionTurnDrafts.first({
        where: { sessionId: active.id },
      }) ?? null
    if (existingDraft) {
      persistTurnDraftAsInterruptedTurn(active.id, existingDraft, active.logger)
      active.nextTurnSequence = resolveLatestStoredTurnSequence(active.id) + 1
    }

    const activeTurn: ActiveTurnBuffer<SessionTurnDraftDoc["id"]> = {
      turnId: randomUUID(),
      sequence: active.nextTurnSequence,
      promptRequestId: nextPrompt.requestId,
      startedAt: new Date().toISOString(),
      messages: [],
      inboxScope: null,
      inboxHeadline: null,
      flushTimer: null,
      draftId: null,
      touchedAttentionEntity: false,
    }
    active.activeTurn = activeTurn
    // Claim the blocking slot before the write so overlapping prompt dispatches stay serialized.
    active.blockingPromptRequestId = nextPrompt.requestId

    if (nextPrompt.resolve || nextPrompt.reject) {
      active.pendingPrompts.set(nextPrompt.requestId, {
        resolve: nextPrompt.resolve ?? (() => {}),
        reject: nextPrompt.reject ?? (() => {}),
      })
    }

    refreshIdleShutdownState(active.id, "turn_started")

    try {
      emitDiagnostic(
        active.id,
        "session_turn_started",
        {
          turnId: activeTurn.turnId,
          sequence: activeTurn.sequence,
          promptRequestId: activeTurn.promptRequestId,
        },
        active.logger,
      )
      await writeImmediateMessage(active, message, {
        persistTurnMessage: false,
        onBeforePublish: async (resolvedMessage) => {
          appendSessionHistoryMessage(activeTurn.messages, resolvedMessage)
          flushActiveTurnDraft(active, "start")
        },
      })
    } catch (error) {
      if (activeTurn.draftId) {
        db.sessionTurnDrafts.delete(activeTurn.draftId)
      }
      clearTurnDraftFlushTimer(active.activeTurn)
      active.activeTurn = null
      if (active.blockingPromptRequestId === nextPrompt.requestId) {
        active.blockingPromptRequestId = null
      }
      active.pendingPrompts.delete(nextPrompt.requestId)
      refreshIdleShutdownState(active.id, "turn_start_failed")
      nextPrompt.reject?.(error instanceof Error ? error : new Error(getErrorMessage(error)))
      throw error
    }
  }

  async function abortQueuedPrompts(
    active: ActiveSession,
    reason: string,
    options: {
      includePendingSteer?: boolean
    } = {},
  ): Promise<AbortedSessionPrompt[]> {
    const abortedQueue: AbortedSessionPrompt[] = []

    if (options.includePendingSteer && active.pendingSteer) {
      const pendingSteer = active.pendingSteer
      active.pendingSteer = null
      abortedQueue.push(
        toAbortedQueuedPrompt({
          requestId: pendingSteer.requestId,
          prompt: pendingSteer.prompt,
        }),
      )
      pendingSteer.reject(new IpcClientError(reason))
    }

    while (active.promptQueue.length > 0) {
      const queuedPrompt = active.promptQueue.shift()!
      abortedQueue.push(toAbortedQueuedPrompt(queuedPrompt))
      if (queuedPrompt.source === "client") {
        // Raw ACP callers need a terminal JSON-RPC response because this prompt never reached the agent.
        publishSessionMessage(active, {
          jsonrpc: "2.0",
          id: queuedPrompt.requestId,
          error: {
            code: QUEUED_PROMPT_ABORTED_ERROR_CODE,
            message: QUEUED_PROMPT_ABORTED_ERROR_MESSAGE,
          },
        })
        continue
      }

      queuedPrompt.reject?.(new IpcClientError(reason))
    }

    refreshIdleShutdownState(active.id, "queued_prompts_aborted")
    return abortedQueue
  }

  async function sendInternalCancel(
    active: ActiveSession,
    options: {
      updateStatus: boolean
    },
  ): Promise<boolean> {
    if (active.blockingPromptRequestId === null) {
      return false
    }

    await writeImmediateMessage(
      active,
      {
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_cancel,
        params: {
          sessionId: active.acpSessionId,
        },
      },
      { updateStatus: options.updateStatus },
    )

    return true
  }

  async function cancelSessionTurn(
    id: SessionId,
    options: {
      includePendingSteer?: boolean
      updateStatus: boolean
    } = { updateStatus: true },
  ): Promise<CancelSessionResponse> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    const abortedQueue = await abortQueuedPrompts(
      active,
      `Queued prompts were aborted for session ${id}.`,
      {
        includePendingSteer: options.includePendingSteer ?? true,
      },
    )
    const activeTurnCancelled = await sendInternalCancel(active, {
      updateStatus: options.updateStatus,
    })

    emitDiagnostic(id, "session_turn_cancelled", {
      activeTurnCancelled,
      abortedQueueLength: abortedQueue.length,
    })

    return {
      id,
      activeTurnCancelled,
      abortedQueue,
    }
  }

  async function handleSteerBoundary(
    active: ActiveSession,
    message: acp.AnyMessage,
  ): Promise<void> {
    const steer = active.pendingSteer
    if (!steer?.waitingForBoundary) {
      return
    }

    const reachedBoundary = isAcpRequest<SessionUpdateMessage>(
      message,
      acp.CLIENT_METHODS.session_update,
    )
      ? message.params.update.sessionUpdate === "tool_call" ||
        message.params.update.sessionUpdate === "tool_call_update"
      : "id" in message && message.id != null && message.id === steer.cancelledRequestId
    if (!reachedBoundary) {
      return
    }

    steer.waitingForBoundary = false
    if (active.blockingPromptRequestId === steer.cancelledRequestId) {
      active.blockingPromptRequestId = null
    }

    active.pendingSteer = null
    try {
      const response = await promptSession(active.id, steer.prompt)
      steer.resolve({
        id: active.id,
        abortedQueue: steer.abortedQueue,
        response,
      })
    } catch (error) {
      refreshIdleShutdownState(active.id, "steer_cleared")
      steer.reject(error instanceof Error ? error : new Error(getErrorMessage(error)))
    }
  }

  async function completeOneShotLaunch(params: {
    id: SessionId
    agentProcess: AgentProcessHandle
    sessionLogger: ReturnType<typeof createLogger>
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

    updateSession(
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
    await treeKill(params.agentProcess)
    await waitForAgentProcessExit(params.agentProcess)

    const sessionDocument = db.sessions.get(params.id) ?? null
    if (!sessionDocument) {
      throw new IpcClientError("Session not found")
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
    sessionLogger: ReturnType<typeof createLogger>
    systemPrompt: string
  }) {
    const connection = createAgentConnection(
      params.agentProcess.stdin,
      params.agentProcess.stdout,
      {
        onChunk: (chunk) => {
          if (chunk.byteLength === 0) {
            return
          }

          params.sessionLogger.log("agent.chunk_read", {
            sessionId: params.id,
            acpSessionId: params.initialized.acpSessionId,
            preview: createChunkPreview(chunk),
          })
        },
        onMessageError: (error) => {
          params.sessionLogger.log("agent.message_handler_failed", {
            errorMessage: getErrorMessage(error),
          })
        },
      },
    )
    const writer = connection.getWriter()
    const activeSession: ActiveSession = {
      id: params.id,
      acpSessionId: params.initialized.acpSessionId,
      logger: params.sessionLogger,
      token: params.token,
      supportsLoadSession: params.supportsLoadSession,
      process: params.agentProcess,
      writer,
      subscription: { close: async () => {} },
      status: params.initialized.status,
      nextTurnSequence: params.nextTurnSequence,
      activeTurn: null,
      isFirstPrompt: params.initialized.isFirstPrompt,
      systemPrompt: params.systemPrompt,
      lastPermissionRequest: null,
      clientRequests: new Map(),
      pendingPrompts: new Map(),
      promptQueue: [],
      blockingPromptRequestId: null,
      pendingSteer: null,
      idleShutdownTimer: null,
    }

    activeSession.subscription = connection.subscribe(async (message) => {
      logAgentMessage(
        activeSession.logger,
        "agent.message_read",
        activeSession.id,
        activeSession.acpSessionId,
        message,
      )
      if (isAcpRequest<PermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)) {
        activeSession.lastPermissionRequest = message
        refreshIdleShutdownState(activeSession.id, "permission_request_started")
      }

      if ("id" in message && message.id != null) {
        const clientRequest = activeSession.clientRequests.get(message.id)
        const promptRequest = clientRequest
          ? matchAcpRequest<acp.PromptRequest>(clientRequest, acp.AGENT_METHODS.session_prompt)
          : null
        const promptResponse = promptRequest
          ? getAcpMessageResult<acp.PromptResponse>(message)
          : null
        const stopReason = promptResponse?.stopReason ?? null
        const nextStatus = stopReason === "end_turn" ? "done" : null

        if (nextStatus || stopReason) {
          updateSession(
            activeSession.id,
            {
              ...(nextStatus && { status: nextStatus }),
              ...(stopReason && { stopReason }),
            },
            {
              reason: "agent_message",
              requestMethod: clientRequest?.method,
              responseId: message.id,
              stopReason: stopReason ?? undefined,
            },
          )
        }
        if (clientRequest) {
          activeSession.clientRequests.delete(message.id)
        }
        settlePendingPrompt(activeSession, message)

        if (message.id === activeSession.blockingPromptRequestId) {
          activeSession.blockingPromptRequestId = null
        }
      }

      publishSessionMessage(activeSession, message)
      finalizeActiveTurn(activeSession, message)
      await handleSteerBoundary(activeSession, message)
      await processPromptQueue(activeSession)
    })

    const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
      try {
        flushActiveTurnDraft(activeSession, "agent_process_exit")
      } catch {}
      cancelIdleShutdownTimer(activeSession, "agent_process_exit")
      activeSessions.delete(activeSession.id)
      await stopReviewSessionRuntime(activeSession.id)
      rejectPendingPrompts(
        activeSession,
        new Error(`Session ${activeSession.id} ended before the prompt completed.`),
      )
      await activeSession.writer.close().catch(() => {})
      await activeSession.subscription.close().catch(() => {})

      const worktreeRecord = await resolvePersistedWorktreeRecord(activeSession.id)
      if (worktreeRecord) {
        const mountedReviewSessionState = await readReviewSessionState(worktreeRecord).catch(
          () => null,
        )
        if (mountedReviewSessionState) {
          try {
            const stopped = await stopReviewSession({ cwd: worktreeRecord.worktreeDir })
            emitDiagnostic(
              activeSession.id,
              "review_session.unmounted",
              {
                reason: "agent_process_exit",
                reviewSessionId: stopped.sessionId,
              },
              activeSession.logger,
            )
          } catch (error) {
            emitDiagnostic(
              activeSession.id,
              "review_session.warning",
              {
                reason: "agent_process_exit",
                errorMessage: getErrorMessage(error),
              },
              activeSession.logger,
            )
          }
        }
      }

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
          updateSession(activeSession.id, nextUpdate, {
            reason: "agent_process_exit",
            code,
            signal,
          })
        } catch {}
      }
    }

    params.agentProcess.onceExit((code, signal) => {
      void handleExit(code, signal)
    })

    activeSessions.set(activeSession.id, activeSession)
    refreshIdleShutdownState(activeSession.id, "session_activated")
    const sessionDocument = db.sessions.get(params.id) ?? null
    if (!sessionDocument) {
      throw new IpcClientError("Session not found")
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
    const existingArtifacts = resolveExistingSessionArtifacts(id, existingSession)
    const resolvedConfig =
      params.config ??
      (input.configManager
        ? (await input.configManager.getRootConfig(params.request.cwd)).config
        : undefined)
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
    const resolvedRegistry = resolvedConfig?.registry
    const worktree = await resolveLaunchWorktree({
      sessionId: id,
      request: resolvedRequest,
      existingWorktree: existingArtifacts.worktree,
      worktreePlugins: resolvedWorktreePlugins,
      defaultWorktreesFolder: resolvedConfig?.worktrees?.defaultFolder,
    })
    const cwd = worktree?.state.effectiveCwd ?? resolvedRequest.cwd
    const sessionMetadata = mergeSessionMetadata(
      existingSession?.metadata,
      resolvedRequest.metadata,
    )
    const existingWorkforceMetadata = existingArtifacts.workforceRecord
      ? omit(existingArtifacts.workforceRecord, ["id", "sessionId"])
      : undefined
    const workforceMetadata = resolvedRequest.workforce ?? existingWorkforceMetadata
    const sessionContext = buildSessionContext({
      sessionId: id,
      request: resolvedRequest,
      cwd,
      worktree,
    })

    const sessionLogContext = buildSessionLogContext({
      request: resolvedRequest,
      cwd,
      workforce: workforceMetadata ?? undefined,
      extraContext: worktree
        ? {
            worktreeDir: worktree.state.worktreeDir,
            worktreePoweredBy: worktree.state.poweredBy,
          }
        : undefined,
    })

    const scope = parseRepoScope(resolvedRequest)
    const reviewSessionEnabled =
      worktree && resolvedRequest.worktree?.reviewSession?.enabled === true

    const nextPermission = {
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    }

    let sessionLogger = logger
    sessionLogger = SessionContext.run(sessionContext, () => sessionLogger.snapshot())
    let mountedReviewSessionState: ReviewSyncStatusData | null = null
    let spawnedAgentProcess: AgentProcessHandle | null = null

    try {
      sessionLogger.log("session.launch_requested", {
        sessionId: id,
        ...sessionLogContext,
      })

      if (
        worktree &&
        !existingArtifacts.worktree &&
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

      if (worktree && reviewSessionEnabled) {
        mountedReviewSessionState = await mountReviewSessionForWorktree(
          id,
          worktree.state,
          sessionLogger,
        )
      }

      const onMessageWrite = (message: acp.AnyMessage) => {
        sessionLogger.log("agent.message_write", {
          direction: "write",
          hasId: "id" in message && message.id != null,
          method: "method" in message ? message.method : undefined,
          message: createPayloadPreview(message),
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
            ...resolvedRequest,
            cwd,
            launchLeaseId: undefined,
            localCheckout: undefined,
            metadata: sessionMetadata,
          },
          onMessageWrite,
        })
      } else {
        agentProcess = await spawnAgentProcess({
          daemonUrl: input.daemonUrl,
          token,
          agent: resolvedRequest.agent,
          cwd,
          agentBinDir: input.agentBinDir,
          env: resolvedRequest.env,
          registryService: input.registryService,
          registry: resolvedRegistry,
        })
        spawnedAgentProcess = agentProcess

        initialized = await initializeSession({
          input: agentProcess.stdin,
          output: agentProcess.stdout,
          request: {
            ...resolvedRequest,
            cwd,
            launchLeaseId: undefined,
            localCheckout: undefined,
            metadata: sessionMetadata,
          },
          resumeAcpId: existingSession?.acpSessionId,
          onMessageWrite,
        })
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

      persistLaunchedSession({
        id,
        existingSession,
        initialTurn,
        existingWorktreeRecord: existingArtifacts.worktreeRecord,
        existingWorkforceRecord: existingArtifacts.workforceRecord,
        worktree,
        workforceMetadata,
        sessionRecord,
      })
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
        queueSessionTitleGeneration({
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
          agentProcess,
          sessionLogger,
          supportsLoadSession: sessionSupportsLoad,
        })
        if (mountedReviewSessionState && worktree) {
          await stopReviewSession({ cwd: worktree.state.worktreeDir }).catch(() => {})
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
        systemPrompt: params.request.systemPrompt,
      })
      if (mountedReviewSessionState && worktree) {
        await startReviewSessionRuntime(id, worktree.state, sessionLogger)
      }
      return liveSession
    } catch (error) {
      sessionLogger.log("session.launch_failed", {
        sessionId: id,
        ...sessionLogContext,
        errorMessage: getErrorMessage(error),
      })
      if (spawnedAgentProcess && !activeSessions.has(id)) {
        await treeKill(spawnedAgentProcess).catch(() => {})
        await waitForAgentProcessExit(spawnedAgentProcess).catch(() => {})
      }
      if (mountedReviewSessionState && worktree) {
        await stopReviewSessionRuntime(id)
        await stopReviewSession({ cwd: worktree.state.worktreeDir }).catch(() => {})
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
      throw new IpcClientError(`Cannot load unknown session: ${params.id}`)
    }

    return launchSession(params, existingSession)
  }

  async function getSession(id: SessionId): Promise<DaemonSession> {
    await ready
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw new IpcClientError("Session not found")
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

    throw new IpcClientError(
      session.connectionMode === "history"
        ? `Session ${id} is archived and no longer reconnectable`
        : `Session ${id} is not reconnectable`,
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
      throw new IpcClientError("Invalid session history cursor")
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

    emitDiagnostic(params.id, "session_history_read", {
      persistedTurnCount: page.items.length,
      returnedTurnCount: turns.length,
      hasCursor: params.cursor != null,
      hasMore: page.next != null,
    })

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
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
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

    if (params.trigger === "at") {
      return {
        suggestions: await getDraftComposerSuggestions({
          cwd: session.cwd,
          trigger: params.trigger,
          query: params.query,
          limit,
        }),
      }
    }

    if (params.trigger === "dollar") {
      return {
        suggestions: await getDraftComposerSuggestions({
          cwd: session.cwd,
          trigger: params.trigger,
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
        trigger: params.trigger,
        query: params.query,
        limit: normalizeComposerSuggestionLimit(params.limit),
      }),
    }
  }

  async function getLaunchPreview(
    params: SessionLaunchPreviewRequest,
  ): Promise<SessionLaunchPreviewResponse> {
    await ready

    const key = createLaunchLeaseKey(params)
    const [repoRoot, branches, dirty] = await Promise.all([
      resolveGitRepoRoot(params.cwd),
      listLaunchBranches(params.cwd),
      inspectLaunchCheckoutDirty(params.cwd),
    ])
    const existingLease = launchLeaseStore.findByKey(key)
    if (existingLease) {
      launchLeaseStore.reactivate(existingLease)
      existingLease.repoRoot = repoRoot
      existingLease.branches = branches
      existingLease.dirty = dirty
      return {
        launchLeaseId: existingLease.id,
        repoRoot,
        branches,
        dirty,
        models: existingLease.models,
        configOptions: existingLease.configOptions,
        slashCommands: getSlashComposerSuggestions(
          existingLease.availableCommands,
          "",
          MAX_COMPOSER_SUGGESTION_LIMIT,
        ),
      }
    }

    const resolvedConfig = await input.configManager
      .getRootConfig(params.cwd)
      .then((root) => root.config)
    const resolvedRegistry = resolvedConfig?.registry
    const launchLeaseId = randomUUID()
    const token = randomBytes(32).toString("hex")
    const agentProcess = await spawnAgentProcess({
      daemonUrl: input.daemonUrl,
      token,
      agent: params.agent,
      cwd: params.cwd,
      agentBinDir: input.agentBinDir,
      registryService: input.registryService,
      registry: resolvedRegistry,
    })
    let availableCommands: acp.AvailableCommand[] = []
    let acpSessionId: string | null = null
    let client: Awaited<ReturnType<typeof createAcpClient>> | null = null
    let resolveAvailableCommands: (() => void) | null = null
    const history: acp.AnyMessage[] = []
    const availableCommandsReady = new Promise<void>((resolve) => {
      resolveAvailableCommands = resolve
    })

    try {
      client = await createAcpClient({
        stdin: agentProcess.stdin,
        stdout: agentProcess.stdout,
        clientInfo: {
          name: "npm:@goddard-ai/daemon",
          version: getPackageVersion(),
        },
        handler: {
          async requestPermission() {
            return { outcome: { outcome: "cancelled" } }
          },
          async sessionUpdate(params) {
            history.push({
              jsonrpc: "2.0",
              method: acp.CLIENT_METHODS.session_update,
              params,
            })
            if (params.update.sessionUpdate === "available_commands_update") {
              availableCommands = params.update.availableCommands
              resolveAvailableCommands?.()
              resolveAvailableCommands = null
            }
          },
        },
      })
      const session = await client.newSession({
        cwd: params.cwd,
        mcpServers: [],
      })
      acpSessionId = session.sessionId

      await Promise.race([
        availableCommandsReady,
        new Promise<void>((resolve) => {
          setTimeout(resolve, 120)
        }),
      ])

      const lease: LaunchLease = {
        id: launchLeaseId,
        key,
        agent: params.agent,
        cwd: params.cwd,
        token,
        acpSessionId,
        agentProcess,
        client,
        session,
        initializeResult: client.initialize,
        history,
        availableCommands,
        models: session.models ?? null,
        configOptions: session.configOptions ?? [],
        repoRoot,
        branches,
        dirty,
        releaseTimer: null,
        closing: null,
      }
      launchLeaseStore.register(lease)

      return {
        launchLeaseId: lease.id,
        repoRoot: lease.repoRoot,
        branches: lease.branches,
        dirty: lease.dirty,
        models: lease.models,
        configOptions: lease.configOptions,
        slashCommands: getSlashComposerSuggestions(
          lease.availableCommands,
          "",
          MAX_COMPOSER_SUGGESTION_LIMIT,
        ),
      }
    } catch (error) {
      if (acpSessionId && client) {
        try {
          await client.closeSession({
            sessionId: acpSessionId,
          })
        } catch {
          // The launch lease failed before it could be returned to a caller.
        }
      }

      await client?.close().catch(() => {})
      await treeKill(agentProcess).catch(() => {})
      await waitForAgentProcessExit(agentProcess).catch(() => {})
      throw error
    }
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

  async function getSubpackages(
    params: SessionSubpackagesRequest,
  ): Promise<SessionSubpackagesResponse> {
    await ready

    const config = await input.configManager.getRootConfig(params.cwd).then((root) => root.config)

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
      events: (diagnosticsRecord?.events ?? []).map((event) => ({
        ...event,
        sessionId: session.id,
      })),
    }
  }

  async function getWorktree(id: SessionId): Promise<GetSessionWorktreeResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: worktreeRecord
        ? toSessionWorktreeValue(worktreeRecord, await resolveReviewSessionState(worktreeRecord))
        : null,
    }
  }

  async function mountReviewSession(id: SessionId): Promise<MutateSessionReviewSessionResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (!worktreeRecord) {
      throw new IpcClientError(`Session ${id} does not have a daemon worktree`)
    }

    const diagnosticLogger = activeSessions.get(id)?.logger ?? logger
    await mountReviewSessionForWorktree(id, worktreeRecord, diagnosticLogger)
    if (activeSessions.has(id)) {
      await startReviewSessionRuntime(id, worktreeRecord, diagnosticLogger)
    }

    const response = await getWorktree(id)
    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: response.worktree,
      warnings: [],
    }
  }

  async function runReviewSession(id: SessionId): Promise<MutateSessionReviewSessionResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (!worktreeRecord) {
      throw new IpcClientError(`Session ${id} does not have a daemon worktree`)
    }

    const diagnosticLogger = activeSessions.get(id)?.logger ?? logger
    emitDiagnostic(id, "review_session.requested", { reason: "manual" }, diagnosticLogger)
    const result = await runReviewSessionCycle(id, worktreeRecord, "manual", diagnosticLogger)
    const response = await getWorktree(id)
    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: response.worktree,
      warnings: result.warnings,
    }
  }

  async function unmountReviewSession(id: SessionId): Promise<MutateSessionReviewSessionResponse> {
    await ready
    const session = await getSession(id)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (!worktreeRecord) {
      throw new IpcClientError(`Session ${id} does not have a daemon worktree`)
    }

    await stopReviewSessionRuntime(id)
    const diagnosticLogger = activeSessions.get(id)?.logger ?? logger
    const state = await readReviewSessionState(worktreeRecord)
    const result = await stopReviewSession({ cwd: worktreeRecord.worktreeDir })
    emitDiagnostic(
      id,
      "review_session.unmounted",
      {
        reason: "manual",
        reviewSessionId: result.sessionId ?? state?.sessionId,
      },
      diagnosticLogger,
    )

    const response = await getWorktree(id)
    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: response.worktree,
      warnings: [],
    }
  }

  async function getWorkforce(id: SessionId): Promise<GetSessionWorkforceResponse> {
    await ready
    const session = await getSession(id)
    const workforceRecord =
      db.workforces.first({
        where: { sessionId: id },
      }) ?? null

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      workforce: workforceRecord,
    }
  }

  async function declareInitiative(id: SessionId, title: string) {
    await ready
    requireSessionDocument(id)
    updateSession(id, {
      status: "active",
      completedHidden: false,
      initiative: title,
      blockedReason: null,
    })

    return getSession(id)
  }

  async function reportBlocker(
    id: SessionId,
    reason: string,
    metadata: SessionInboxMetadataInput = {},
  ) {
    await ready
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata: {
        ...metadata,
        fallbackHeadline: reason,
      },
      blockedReason: reason,
    })
    updateSession(id, {
      status: "blocked",
      completedHidden: false,
      blockedReason: reason,
    })
    input.inboxManager.touchInboxItem({
      entityId: id,
      reason: "session.blocked",
      scope: resolved.scope,
      headline: resolved.headline,
      turnId: resolveCurrentTurnId(id),
    })

    return getSession(id)
  }

  async function reportTurnEnded(id: SessionId, metadata: SessionInboxMetadataInput = {}) {
    await ready
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata: {
        ...metadata,
        fallbackHeadline: session.lastAgentMessage ?? session.initiative ?? session.title,
      },
    })
    updateSession(id, {
      status: "done",
      completedHidden: false,
      initiative: null,
      blockedReason: null,
    })

    const activeTurn = activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn?.touchedAttentionEntity !== true) {
      input.inboxManager.touchInboxItem({
        entityId: id,
        reason: "session.turn_ended",
        scope: resolved.scope,
        headline: resolved.headline,
        turnId: resolveCurrentTurnId(id),
      })
    }

    return getSession(id)
  }

  async function recordTurnAttentionActivity(
    id: SessionId,
    metadata: SessionInboxMetadataInput & { fallbackHeadline?: string } = {},
  ) {
    await ready
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata,
    })
    const activeTurn = activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      activeTurn.touchedAttentionEntity = true
    }

    return {
      scope: resolved.scope,
      headline: resolved.headline,
      turnId: resolveCurrentTurnId(id),
    }
  }

  async function completeSession(id: SessionId) {
    await ready
    requireSessionDocument(id)
    const active = activeSessions.get(id) ?? null
    if (active?.activeTurn) {
      throw new IpcClientError("Cannot complete a session while the agent has an active turn")
    }

    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (worktreeRecord) {
      let completionState: Awaited<ReturnType<typeof inspectWorktreeCompletionState>>
      try {
        completionState = await inspectWorktreeCompletionState(worktreeRecord)
      } catch {
        throw new IpcClientError(
          "Cannot complete a worktree session because its git state could not be inspected",
        )
      }

      if (completionState.dirty) {
        throw new IpcClientError(
          "Cannot complete a worktree session while its working tree has uncommitted changes",
        )
      }

      if (completionState.unmergedCommits) {
        throw new IpcClientError(
          "Cannot complete a worktree session while it has commits that have not been merged into the primary checkout",
        )
      }
    }

    updateSession(id, {
      completedHidden: true,
    })
    return input.inboxManager.completeSession(id)
  }

  async function sendMessage(id: SessionId, message: acp.AnyMessage): Promise<void> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    if (isAcpRequest<PromptRequestMessage>(message, acp.AGENT_METHODS.session_prompt)) {
      if ("id" in message === false || message.id == null) {
        throw new IpcClientError("Queued prompt messages must include a JSON-RPC id")
      }

      queueSessionTitlePreparation({
        id: active.id,
        prompt: message.params.prompt,
        diagnosticLogger: active.logger,
      })
      active.promptQueue.push({
        requestId: message.id,
        prompt: [...message.params.prompt],
        source: "client",
      })
      updateSession(id, {
        completedHidden: false,
      })
      input.inboxManager.markSessionReplied(id)
      refreshIdleShutdownState(active.id, "prompt_enqueued")
      emitDiagnostic(active.id, "session_prompt_enqueued", {
        requestId: message.id,
        queueLength: active.promptQueue.length,
      })
      await processPromptQueue(active)
      return
    }

    if (isAcpRequest(message, acp.AGENT_METHODS.session_cancel)) {
      await abortQueuedPrompts(active, `Queued prompts were aborted for session ${id}.`, {
        includePendingSteer: true,
      })
      await writeImmediateMessage(active, message)
      return
    }

    await writeImmediateMessage(active, message)
  }

  async function promptSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<acp.PromptResponse> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    queueSessionTitlePreparation({
      id: active.id,
      prompt,
      diagnosticLogger: active.logger,
    })
    const requestId = randomUUID()
    const response = new Promise<acp.PromptResponse>((resolve, reject) => {
      active.promptQueue.push({
        requestId,
        prompt: normalizePrompt(prompt),
        source: "daemon",
        resolve,
        reject,
      })
    })

    try {
      refreshIdleShutdownState(active.id, "prompt_enqueued")
      emitDiagnostic(
        active.id,
        "session_prompt_enqueued",
        {
          requestId,
          queueLength: active.promptQueue.length,
        },
        active.logger,
      )
      await processPromptQueue(active)
      return await response
    } catch (error) {
      active.pendingPrompts.delete(requestId)
      throw error
    }
  }

  async function steerSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<SteerSessionResponse> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    const requestId = randomUUID()
    const abortedQueue = await abortQueuedPrompts(
      active,
      `Queued prompts were aborted for session ${id}.`,
      {
        includePendingSteer: true,
      },
    )

    if (active.blockingPromptRequestId === null) {
      const response = await promptSession(id, prompt)
      return {
        id,
        abortedQueue,
        response,
      }
    }

    return await new Promise<SteerSessionResponse>((resolve, reject) => {
      // Keep tracking the cancelled prompt id so steering can wait for that turn's tool/final boundary.
      active.pendingSteer = {
        requestId,
        cancelledRequestId: active.blockingPromptRequestId!,
        prompt: normalizePrompt(prompt),
        abortedQueue,
        waitingForBoundary: true,
        resolve,
        reject,
      }
      refreshIdleShutdownState(active.id, "steer_started")

      void sendInternalCancel(active, { updateStatus: false }).catch((error) => {
        if (active.pendingSteer?.requestId === requestId) {
          active.pendingSteer = null
        }
        reject(error instanceof Error ? error : new Error(getErrorMessage(error)))
      })
    })
  }

  async function shutdownSession(id: SessionId): Promise<boolean> {
    await ready
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    cancelIdleShutdownTimer(active, "session_shutdown")
    emitDiagnostic(id, "session_shutdown_requested", undefined, active.logger)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (worktreeRecord) {
      const mountedReviewSessionState = await readReviewSessionState(worktreeRecord).catch(
        () => null,
      )
      if (mountedReviewSessionState) {
        try {
          await stopReviewSessionRuntime(id)
          const result = await stopReviewSession({ cwd: worktreeRecord.worktreeDir })
          emitDiagnostic(
            id,
            "review_session.unmounted",
            {
              reason: "session_shutdown",
              reviewSessionId: result.sessionId ?? mountedReviewSessionState.sessionId,
            },
            active.logger,
          )
        } catch (error) {
          emitDiagnostic(
            id,
            "review_session.warning",
            {
              reason: "session_shutdown",
              errorMessage: getErrorMessage(error),
            },
            active.logger,
          )
          return false
        }
      }
    }
    await treeKill(active.process)
    await waitForAgentProcessExit(active.process)
    return true
  }

  async function resolveSessionIdByToken(token: string): Promise<SessionId> {
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
    await launchLeaseStore.closeAll("daemon_shutdown")

    for (const session of activeSessions.values()) {
      cancelIdleShutdownTimer(session, "daemon_shutdown")
      await stopReviewSessionRuntime(session.id)
      const worktreeRecord = await resolvePersistedWorktreeRecord(session.id)
      if (worktreeRecord) {
        if (await readReviewSessionState(worktreeRecord).catch(() => null)) {
          await stopReviewSession({ cwd: worktreeRecord.worktreeDir }).catch(() => {})
        }
      }
      emitDiagnostic(session.id, "daemon_shutdown", { status: session.status }, session.logger)
      await treeKill(session.process)
      await waitForAgentProcessExit(session.process)
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
    getChanges,
    getComposerSuggestions,
    getDraftSuggestions,
    getLaunchPreview,
    releaseLaunchLease,
    getSubpackages,
    getDiagnostics,
    getWorktree,
    mountReviewSession,
    runReviewSession,
    unmountReviewSession,
    getWorkforce,
    declareInitiative,
    reportBlocker,
    reportTurnEnded,
    recordTurnAttentionActivity,
    completeSession,
    sendMessage,
    cancelSessionTurn,
    steerSession,
    promptSession,
    shutdownSession,
    sessionSubscriberConnected,
    sessionSubscriberDisconnected,
    resolveSessionIdByToken,
    close,
  }
}
