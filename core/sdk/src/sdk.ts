import type * as acp from "@agentclientprotocol/sdk"
import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type { DeviceFlowComplete, DeviceFlowStart } from "@goddard-ai/schema/backend"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/common/params"
import type {
  BulkUpdateInboxItemsRequest,
  CancelSessionRequest,
  CancelWorkforceRequest,
  CreateSessionRequest,
  CreateWorkforceRequest,
  DeclareSessionInitiativeRequest,
  DiscoverWorkforceCandidatesRequest,
  GetLoopRequest,
  GetPullRequestRequest,
  GetSessionChangesRequest,
  GetSessionHistoryRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  ListAdaptersRequest,
  ListInboxRequest,
  ListSessionsRequest,
  MountSessionWorktreeSyncRequest,
  ReplyPrRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  RespondWorkforceRequest,
  RunNamedActionRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  ShutdownLoopRequest,
  ShutdownWorkforceRequest,
  StartLoopRequest,
  StartWorkforceRequest,
  SteerSessionRequest,
  SubmitPrRequest,
  SubscribeWorkforceEventsRequest,
  SuspendWorkforceRequest,
  SyncSessionWorktreeRequest,
  TruncateWorkforceRequest,
  UnmountSessionWorktreeSyncRequest,
  UpdateInboxItemRequest,
  UpdateWorkforceRequest,
  WorkforceEventEnvelope,
} from "@goddard-ai/schema/daemon"

import { runSession } from "./daemon/session/client.ts"
import { resolveIpcClient, type IpcClientOptions } from "./ipc-client.ts"
import {
  createSessionPromptMessage,
  type SessionParams,
  type SessionPromptRequest,
} from "./session.ts"

/** Constructor options for the browser-safe daemon-backed SDK facade. */
export type GoddardClientOptions = IpcClientOptions

/** Caches one namespace on first access by replacing the instance getter with the concrete value. */
function defineCachedNamespace<TValue>(owner: object, key: string, value: TValue): TValue {
  Object.defineProperty(owner, key, {
    configurable: true,
    value,
  })
  return value
}

/** Builds the health namespace with one thin method per daemon health IPC action. */
function createDaemonNamespace(client: DaemonIpcClient) {
  return {
    /** Probes daemon liveness without adding SDK-specific behavior. */
    health: async () => client.send("daemon.health"),
  }
}

/** Builds the auth namespace with one thin method per daemon auth IPC action. */
function createAuthNamespace(client: DaemonIpcClient) {
  return {
    /** Starts one GitHub device flow through the daemon auth contract. */
    startDeviceFlow: async (input: DeviceFlowStart) => client.send("auth.device.start", input),

    /** Completes one pending GitHub device flow through the daemon auth contract. */
    completeDeviceFlow: async (input: DeviceFlowComplete) =>
      client.send("auth.device.complete", input),

    /** Reads the current daemon-owned auth session as-is. */
    whoami: async () => client.send("auth.whoami"),

    /** Clears the current daemon-owned auth session as-is. */
    logout: async () => client.send("auth.logout"),
  }
}

/** Builds the adapter namespace with one thin method per daemon adapter IPC action. */
function createAdapterNamespace(client: DaemonIpcClient) {
  return {
    /** Lists adapters available for one project or global launch flow. */
    list: async (input: ListAdaptersRequest = {}) => client.send("adapter.list", input),
  }
}

/** Builds the pull request namespace with one thin method per daemon PR IPC action. */
function createPrNamespace(client: DaemonIpcClient) {
  return {
    /** Submits one pull request through the daemon PR contract. */
    submit: async (input: SubmitPrRequest & { token: string }) => client.send("pr.submit", input),

    /** Fetches one daemon-managed pull request by tagged id. */
    get: async (input: GetPullRequestRequest) => client.send("pr.get", input),

    /** Posts one pull request reply through the daemon PR contract. */
    reply: async (input: ReplyPrRequest & { token: string }) => client.send("pr.reply", input),
  }
}

/** Builds the inbox namespace with one thin method per daemon inbox IPC action. */
function createInboxNamespace(client: DaemonIpcClient) {
  return {
    /** Lists daemon-local inbox rows using daemon ordering and filtering. */
    list: async (input: ListInboxRequest = {}) => client.send("inbox.list", input),

    /** Updates one daemon-local inbox row by entity id. */
    update: async (input: UpdateInboxItemRequest) => client.send("inbox.update", input),

    /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
    bulkUpdate: async (input: BulkUpdateInboxItemsRequest) =>
      client.send("inbox.bulkUpdate", input),
  }
}

/** Builds the session namespace with one thin method per daemon session IPC action. */
function createSessionNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
    run: async (input: SessionParams, handler?: acp.Client) => runSession(client, input, handler),

    /** Creates one daemon-managed session record. */
    create: async (input: CreateSessionRequest) => client.send("session.create", input),

    /** Lists daemon-managed sessions and pagination state. */
    list: async (input: ListSessionsRequest) => client.send("session.list", input),

    /** Fetches one daemon-managed session record. */
    get: async (input: DaemonSessionIdParams) => client.send("session.get", input),

    /** Reconnects to one daemon-managed session record. */
    connect: async (input: DaemonSessionIdParams) => client.send("session.connect", input),

    /** Reads one daemon-managed session history with session identity and connection state. */
    history: async (input: GetSessionHistoryRequest) => client.send("session.history", input),

    /** Reads the current git diff for one daemon-managed session workspace. */
    changes: async (input: GetSessionChangesRequest) => client.send("session.changes", input),

    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: async (input: SessionComposerSuggestionsRequest) =>
      client.send("session.composerSuggestions", input),

    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: async (input: SessionDraftSuggestionsRequest) =>
      client.send("session.draftSuggestions", input),

    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: async (input: SessionLaunchPreviewRequest) =>
      client.send("session.launchPreview", input),

    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: async (input: DaemonSessionIdParams) => client.send("session.diagnostics", input),

    /** Reads persisted worktree metadata attached to one daemon-managed session. */
    worktree: async (input: DaemonSessionIdParams) => client.send("session.worktree.get", input),

    /** Mounts sync for one daemon-managed session worktree. */
    mountWorktreeSync: async (input: MountSessionWorktreeSyncRequest) =>
      client.send("session.worktreeSync.mount", input),

    /** Asks the daemon to run its normal worktree sync cycle immediately for one mounted worktree. */
    syncWorktree: async (input: SyncSessionWorktreeRequest) =>
      client.send("session.worktreeSync.run", input),

    /** Unmounts sync for one daemon-managed session worktree. */
    unmountWorktree: async (input: UnmountSessionWorktreeSyncRequest) =>
      client.send("session.worktreeSync.unmount", input),

    /** Reads persisted workforce metadata attached to one daemon-managed session. */
    workforce: async (input: DaemonSessionIdParams) => client.send("session.workforce.get", input),

    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: async (input: DaemonSessionIdParams) => client.send("session.shutdown", input),

    /** Marks one session inbox row completed without shutting down the session. */
    complete: async (input: DaemonSessionIdParams) => client.send("session.complete", input),

    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: async (input: DeclareSessionInitiativeRequest) =>
      client.send("session.declareInitiative", input),

    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: async (input: ReportSessionBlockerRequest) =>
      client.send("session.reportBlocker", input),

    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: async (input: ReportSessionTurnEndedRequest) =>
      client.send("session.reportTurnEnded", input),

    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: async (input: CancelSessionRequest) => client.send("session.cancel", input),
    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: async (input: SteerSessionRequest) => client.send("session.steer", input),
    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: async (input: SendSessionMessageRequest) => client.send("session.send", input),
    /** Sends one prompt to a daemon-managed session without exposing raw ACP message construction. */
    prompt: async (input: SessionPromptRequest) =>
      client.send("session.send", {
        id: input.id,
        message: createSessionPromptMessage(input),
      }),
    /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
    subscribe: async (
      input: DaemonSessionIdParams,
      onMessage: (message: acp.AnyMessage) => void,
    ): Promise<() => void> => {
      return client.subscribe({ name: "session.message", filter: input }, (payload) => {
        onMessage(payload.message)
      })
    },

    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: async (input: ResolveSessionTokenRequest) =>
      client.send("session.resolveToken", input),
  }
}

/** Builds the action namespace with one thin method per daemon action IPC call. */
function createActionNamespace(client: DaemonIpcClient) {
  return {
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: async (input: RunNamedActionRequest) => client.send("action.run", input),
  }
}

/** Builds the loop namespace with one thin method per daemon loop IPC action. */
function createLoopNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon loop runtime. */
    start: async (input: StartLoopRequest) => client.send("loop.start", input),

    /** Fetches one daemon loop runtime and its resolved config. */
    get: async (input: GetLoopRequest) => client.send("loop.get", input),

    /** Lists daemon loop runtime summaries. */
    list: async () => client.send("loop.list"),

    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownLoopRequest) => client.send("loop.shutdown", input),
  }
}

/** Builds the workforce namespace with one thin method per daemon workforce IPC action. */
function createWorkforceNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon workforce runtime. */
    start: async (input: StartWorkforceRequest) => client.send("workforce.start", input),

    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: async (input: DiscoverWorkforceCandidatesRequest) =>
      client.send("workforce.discoverCandidates", input),

    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: async (input: InitializeWorkforceRequest) =>
      client.send("workforce.initialize", input),

    /** Fetches one daemon workforce runtime and its resolved config. */
    get: async (input: GetWorkforceRequest) => client.send("workforce.get", input),

    /** Lists daemon workforce runtime summaries. */
    list: async () => client.send("workforce.list"),

    /** Subscribes to live daemon-published workforce ledger events for one repository root. */
    subscribe: async (
      input: SubscribeWorkforceEventsRequest,
      onEvent: (event: WorkforceEventEnvelope["event"]) => void,
    ): Promise<() => void> => {
      return client.subscribe({ name: "workforce.event", filter: input }, (payload) => {
        onEvent(payload.event)
      })
    },

    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownWorkforceRequest) => client.send("workforce.shutdown", input),

    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: async (input: CreateWorkforceRequest) => client.send("workforce.request", input),

    /** Updates one workforce request and includes the updated workforce projection. */
    update: async (input: UpdateWorkforceRequest) => client.send("workforce.update", input),

    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: async (input: CancelWorkforceRequest) => client.send("workforce.cancel", input),

    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: async (input: TruncateWorkforceRequest) => client.send("workforce.truncate", input),

    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: async (input: RespondWorkforceRequest) => client.send("workforce.respond", input),

    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: async (input: SuspendWorkforceRequest) => client.send("workforce.suspend", input),
  }
}

/** Browser-safe SDK facade that mirrors the daemon IPC contract through thin namespace methods. */
export class GoddardSdk {
  readonly #client: DaemonIpcClient

  constructor(options: GoddardClientOptions) {
    this.#client = resolveIpcClient(options)
  }

  get daemon() {
    return defineCachedNamespace(this, "daemon", createDaemonNamespace(this.#client))
  }

  get auth() {
    return defineCachedNamespace(this, "auth", createAuthNamespace(this.#client))
  }

  get adapter() {
    return defineCachedNamespace(this, "adapter", createAdapterNamespace(this.#client))
  }

  get pr() {
    return defineCachedNamespace(this, "pr", createPrNamespace(this.#client))
  }

  get inbox() {
    return defineCachedNamespace(this, "inbox", createInboxNamespace(this.#client))
  }

  get session() {
    return defineCachedNamespace(this, "session", createSessionNamespace(this.#client))
  }

  get action() {
    return defineCachedNamespace(this, "action", createActionNamespace(this.#client))
  }

  get loop() {
    return defineCachedNamespace(this, "loop", createLoopNamespace(this.#client))
  }

  get workforce() {
    return defineCachedNamespace(this, "workforce", createWorkforceNamespace(this.#client))
  }
}
