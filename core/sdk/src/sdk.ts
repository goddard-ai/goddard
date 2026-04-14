import type * as acp from "@agentclientprotocol/sdk"
import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type { DeviceFlowComplete, DeviceFlowStart } from "@goddard-ai/schema/backend"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/common/params"
import type {
  CancelSessionRequest,
  CancelWorkforceRequest,
  CreateSessionRequest,
  CreateWorkforceRequest,
  DiscoverWorkforceCandidatesRequest,
  GetLoopRequest,
  ListAdaptersRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  ListSessionsRequest,
  MountSessionWorktreeSyncRequest,
  ReplyPrRequest,
  ResolveSessionTokenRequest,
  RespondWorkforceRequest,
  RunNamedActionRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
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
    health: async () => client.send("health"),
  }
}

/** Builds the auth namespace with one thin method per daemon auth IPC action. */
function createAuthNamespace(client: DaemonIpcClient) {
  return {
    /** Starts one GitHub device flow through the daemon auth contract. */
    startDeviceFlow: async (input: DeviceFlowStart) => client.send("authDeviceStart", input),

    /** Completes one pending GitHub device flow through the daemon auth contract. */
    completeDeviceFlow: async (input: DeviceFlowComplete) =>
      client.send("authDeviceComplete", input),

    /** Reads the current daemon-owned auth session as-is. */
    whoami: async () => client.send("authWhoami"),

    /** Clears the current daemon-owned auth session as-is. */
    logout: async () => client.send("authLogout"),
  }
}

/** Builds the adapter namespace with one thin method per daemon adapter IPC action. */
function createAdapterNamespace(client: DaemonIpcClient) {
  return {
    /** Lists adapters available for one project or global launch flow. */
    list: async (input: ListAdaptersRequest = {}) => client.send("adapterList", input),
  }
}

/** Builds the pull request namespace with one thin method per daemon PR IPC action. */
function createPrNamespace(client: DaemonIpcClient) {
  return {
    /** Submits one pull request through the daemon PR contract. */
    submit: async (input: SubmitPrRequest & { token: string }) => client.send("prSubmit", input),

    /** Posts one pull request reply through the daemon PR contract. */
    reply: async (input: ReplyPrRequest & { token: string }) => client.send("prReply", input),
  }
}

/** Builds the session namespace with one thin method per daemon session IPC action. */
function createSessionNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
    run: async (input: SessionParams, handler?: acp.Client) => runSession(client, input, handler),

    /** Creates one daemon-managed session record. */
    create: async (input: CreateSessionRequest) => client.send("sessionCreate", input),

    /** Lists daemon-managed sessions and pagination state. */
    list: async (input: ListSessionsRequest) => client.send("sessionList", input),

    /** Fetches one daemon-managed session record. */
    get: async (input: DaemonSessionIdParams) => client.send("sessionGet", input),

    /** Reconnects to one daemon-managed session record. */
    connect: async (input: DaemonSessionIdParams) => client.send("sessionConnect", input),

    /** Reads one daemon-managed session history with session identity and connection state. */
    history: async (input: DaemonSessionIdParams) => client.send("sessionHistory", input),

    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: async (input: SessionComposerSuggestionsRequest) =>
      client.send("sessionComposerSuggestions", input),

    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: async (input: DaemonSessionIdParams) => client.send("sessionDiagnostics", input),

    /** Reads persisted worktree metadata attached to one daemon-managed session. */
    worktree: async (input: DaemonSessionIdParams) => client.send("sessionWorktree", input),

    /** Mounts sync for one daemon-managed session worktree. */
    mountWorktreeSync: async (input: MountSessionWorktreeSyncRequest) =>
      client.send("sessionWorktreeSyncMount", input),

    /** Asks the daemon to run its normal worktree sync cycle immediately for one mounted worktree. */
    syncWorktree: async (input: SyncSessionWorktreeRequest) =>
      client.send("sessionWorktreeSync", input),

    /** Unmounts sync for one daemon-managed session worktree. */
    unmountWorktree: async (input: UnmountSessionWorktreeSyncRequest) =>
      client.send("sessionWorktreeSyncUnmount", input),

    /** Reads persisted workforce metadata attached to one daemon-managed session. */
    workforce: async (input: DaemonSessionIdParams) => client.send("sessionWorkforce", input),

    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: async (input: DaemonSessionIdParams) => client.send("sessionShutdown", input),
    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: async (input: CancelSessionRequest) => client.send("sessionCancel", input),
    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: async (input: SteerSessionRequest) => client.send("sessionSteer", input),
    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: async (input: SendSessionMessageRequest) => client.send("sessionSend", input),
    /** Sends one prompt to a daemon-managed session without exposing raw ACP message construction. */
    prompt: async (input: SessionPromptRequest) =>
      client.send("sessionSend", {
        id: input.id,
        message: createSessionPromptMessage(input),
      }),
    /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
    subscribe: async (
      input: DaemonSessionIdParams,
      onMessage: (message: acp.AnyMessage) => void,
    ): Promise<() => void> => {
      return client.subscribe({ name: "sessionMessage", filter: input }, (payload) => {
        onMessage(payload.message)
      })
    },

    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: async (input: ResolveSessionTokenRequest) =>
      client.send("sessionResolveToken", input),
  }
}

/** Builds the action namespace with one thin method per daemon action IPC call. */
function createActionNamespace(client: DaemonIpcClient) {
  return {
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: async (input: RunNamedActionRequest) => client.send("actionRun", input),
  }
}

/** Builds the loop namespace with one thin method per daemon loop IPC action. */
function createLoopNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon loop runtime. */
    start: async (input: StartLoopRequest) => client.send("loopStart", input),

    /** Fetches one daemon loop runtime and its resolved config. */
    get: async (input: GetLoopRequest) => client.send("loopGet", input),

    /** Lists daemon loop runtime summaries. */
    list: async () => client.send("loopList"),

    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownLoopRequest) => client.send("loopShutdown", input),
  }
}

/** Builds the workforce namespace with one thin method per daemon workforce IPC action. */
function createWorkforceNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon workforce runtime. */
    start: async (input: StartWorkforceRequest) => client.send("workforceStart", input),

    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: async (input: DiscoverWorkforceCandidatesRequest) =>
      client.send("workforceDiscoverCandidates", input),

    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: async (input: InitializeWorkforceRequest) =>
      client.send("workforceInitialize", input),

    /** Fetches one daemon workforce runtime and its resolved config. */
    get: async (input: GetWorkforceRequest) => client.send("workforceGet", input),

    /** Lists daemon workforce runtime summaries. */
    list: async () => client.send("workforceList"),

    /** Subscribes to live daemon-published workforce ledger events for one repository root. */
    subscribe: async (
      input: SubscribeWorkforceEventsRequest,
      onEvent: (event: WorkforceEventEnvelope["event"]) => void,
    ): Promise<() => void> => {
      return client.subscribe({ name: "workforceEvent", filter: input }, (payload) => {
        onEvent(payload.event)
      })
    },

    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownWorkforceRequest) => client.send("workforceShutdown", input),

    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: async (input: CreateWorkforceRequest) => client.send("workforceRequest", input),

    /** Updates one workforce request and includes the updated workforce projection. */
    update: async (input: UpdateWorkforceRequest) => client.send("workforceUpdate", input),

    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: async (input: CancelWorkforceRequest) => client.send("workforceCancel", input),

    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: async (input: TruncateWorkforceRequest) => client.send("workforceTruncate", input),

    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: async (input: RespondWorkforceRequest) => client.send("workforceRespond", input),

    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: async (input: SuspendWorkforceRequest) => client.send("workforceSuspend", input),
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
