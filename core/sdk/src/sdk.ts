import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import {
  InferStreamFilter,
  InferStreamPayload,
  IpcClient,
  IpcSchema,
  RequestArguments,
  StreamTarget,
  ValidRequestName,
  ValidStreamName,
} from "@goddard-ai/ipc"

import { resolveIpcClient, type IpcClientOptions } from "./ipc-client.ts"

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

function defineRequest<S extends IpcSchema, K extends ValidRequestName<S>>(
  client: IpcClient<S>,
  name: K,
) {
  return (...args: RequestArguments<S, K>) => client.send(name, ...args)
}

function defineSubscription<S extends IpcSchema, K extends ValidStreamName<S>>(
  client: IpcClient<S>,
  name: K,
) {
  type SubscribeOverload =
    | ((onMessage: (payload: InferStreamPayload<S, K>) => void) => Promise<() => void>)
    | ((
        onMessage: (payload: InferStreamPayload<S, K>) => void,
        onError: (error: unknown) => void,
      ) => () => void)
    | ((
        onMessage: (payload: InferStreamPayload<S, K>) => void,
        onError?: (error: unknown) => void,
      ) => (() => void) | Promise<() => void>)

  return function subscribe(
    filter: InferStreamFilter<S, K> | ((payload: InferStreamPayload<S, K>) => void),
    onMessage?: ((payload: InferStreamPayload<S, K>) => void) | ((error: unknown) => void),
    onError?: (error: unknown) => void,
  ) {
    if (typeof filter === "function") {
      return client.subscribe(name as StreamTarget<S, K>, filter, onMessage)
    }
    return client.subscribe(
      { name, filter } as StreamTarget<S, K>,
      onMessage as (payload: InferStreamPayload<S, K>) => void,
      onError,
    )
  } as [InferStreamFilter<S, K>] extends [void]
    ? SubscribeOverload
    : SubscribeOverload extends (...args: infer TArgs) => infer TResult
      ? (filter: InferStreamFilter<S, K>, ...args: TArgs) => TResult
      : never
}

/** Builds the health namespace with one thin method per daemon health IPC action. */
function createDaemonNamespace(client: DaemonIpcClient) {
  return {
    /** Probes daemon liveness without adding SDK-specific behavior. */
    health: defineRequest(client, "daemon.health"),
  }
}

/** Builds the auth namespace with one thin method per daemon auth IPC action. */
function createAuthNamespace(client: DaemonIpcClient) {
  return {
    /** Starts one GitHub device flow through the daemon auth contract. */
    startDeviceFlow: defineRequest(client, "auth.device.start"),

    /** Completes one pending GitHub device flow through the daemon auth contract. */
    completeDeviceFlow: defineRequest(client, "auth.device.complete"),

    /** Reads the current daemon-owned auth session as-is. */
    whoami: defineRequest(client, "auth.whoami"),

    /** Clears the current daemon-owned auth session as-is. */
    logout: defineRequest(client, "auth.logout"),
  }
}

/** Builds the adapter namespace with one thin method per daemon adapter IPC action. */
function createAdapterNamespace(client: DaemonIpcClient) {
  return {
    /** Lists adapters available for one project or global launch flow. */
    list: defineRequest(client, "adapter.list"),
  }
}

/** Builds the pull request namespace with one thin method per daemon PR IPC action. */
function createPrNamespace(client: DaemonIpcClient) {
  return {
    /** Submits one pull request through the daemon PR contract. */
    submit: defineRequest(client, "pr.submit"),

    /** Fetches one daemon-managed pull request by tagged id. */
    get: defineRequest(client, "pr.get"),

    /** Posts one pull request reply through the daemon PR contract. */
    reply: defineRequest(client, "pr.reply"),
  }
}

/** Builds the inbox namespace with one thin method per daemon inbox IPC action. */
function createInboxNamespace(client: DaemonIpcClient) {
  return {
    /** Lists daemon-local inbox rows using daemon ordering and filtering. */
    list: defineRequest(client, "inbox.list"),

    /** Updates one daemon-local inbox row by entity id. */
    update: defineRequest(client, "inbox.update"),

    /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
    bulkUpdate: defineRequest(client, "inbox.bulkUpdate"),

    /** Subscribes to daemon-published inbox item updates. */
    subscribe: defineSubscription(client, "inbox.item"),
  }
}

/** Builds the session namespace with one thin method per daemon session IPC action. */
function createSessionNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
    run: defineRequest(client, "session.run"),

    /** Creates one daemon-managed session record. */
    create: defineRequest(client, "session.create"),

    /** Lists daemon-managed sessions and pagination state. */
    list: defineRequest(client, "session.list"),

    /** Fetches one daemon-managed session record. */
    get: defineRequest(client, "session.get"),

    /** Reconnects to one daemon-managed session record. */
    connect: defineRequest(client, "session.connect"),

    /** Reads one daemon-managed session history with session identity and connection state. */
    history: defineRequest(client, "session.history"),

    /** Reads the current git diff for one daemon-managed session workspace. */
    changes: defineRequest(client, "session.changes"),

    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: defineRequest(client, "session.composerSuggestions"),

    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: defineRequest(client, "session.draftSuggestions"),

    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: defineRequest(client, "session.launchPreview"),

    /** Discovers launchable subpackage working directories under one project cwd. */
    subpackages: defineRequest(client, "session.subpackages"),

    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: defineRequest(client, "session.diagnostics"),

    /** Reads persisted worktree metadata attached to one daemon-managed session. */
    worktree: defineRequest(client, "session.worktree.get"),

    /** Mounts a review session for one daemon-managed session worktree. */
    mountReviewSession: defineRequest(client, "session.reviewSession.mount"),

    /** Runs one mounted review session immediately. */
    runReviewSession: defineRequest(client, "session.reviewSession.run"),

    /** Unmounts a review session from one daemon-managed session worktree. */
    unmountReviewSession: defineRequest(client, "session.reviewSession.unmount"),

    /** Reads persisted workforce metadata attached to one daemon-managed session. */
    workforce: defineRequest(client, "session.workforce.get"),

    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: defineRequest(client, "session.shutdown"),

    /** Marks one session inbox row completed without shutting down the session. */
    complete: defineRequest(client, "session.complete"),

    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: defineRequest(client, "session.declareInitiative"),

    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: defineRequest(client, "session.reportBlocker"),

    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: defineRequest(client, "session.reportTurnEnded"),

    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: defineRequest(client, "session.cancel"),

    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: defineRequest(client, "session.steer"),

    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: defineRequest(client, "session.send"),

    /** Sends one ACP permission response through the daemon-managed session transport. */
    respondPermission: defineRequest(client, "session.respondPermission"),

    /** Sends one prompt to a daemon-managed session without exposing raw ACP message construction. */
    prompt: defineRequest(client, "session.prompt"),

    /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
    subscribe: defineSubscription(client, "session.message"),

    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: defineRequest(client, "session.resolveToken"),
  }
}

/** Builds the action namespace with one thin method per daemon action IPC call. */
function createActionNamespace(client: DaemonIpcClient) {
  return {
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: defineRequest(client, "action.run"),
  }
}

/** Builds the loop namespace with one thin method per daemon loop IPC action. */
function createLoopNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon loop runtime. */
    start: defineRequest(client, "loop.start"),

    /** Fetches one daemon loop runtime and its resolved config. */
    get: defineRequest(client, "loop.get"),

    /** Lists daemon loop runtime summaries. */
    list: defineRequest(client, "loop.list"),

    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: defineRequest(client, "loop.shutdown"),
  }
}

/** Builds the workforce namespace with one thin method per daemon workforce IPC action. */
function createWorkforceNamespace(client: DaemonIpcClient) {
  return {
    /** Starts or reuses one daemon workforce runtime. */
    start: defineRequest(client, "workforce.start"),

    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: defineRequest(client, "workforce.discoverCandidates"),

    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: defineRequest(client, "workforce.initialize"),

    /** Fetches one daemon workforce runtime and its resolved config. */
    get: defineRequest(client, "workforce.get"),

    /** Lists daemon workforce runtime summaries. */
    list: defineRequest(client, "workforce.list"),

    /** Subscribes to live daemon-published workforce ledger events for one repository root. */
    subscribe: defineSubscription(client, "workforce.event"),

    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: defineRequest(client, "workforce.shutdown"),

    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: defineRequest(client, "workforce.request"),

    /** Updates one workforce request and includes the updated workforce projection. */
    update: defineRequest(client, "workforce.update"),

    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: defineRequest(client, "workforce.cancel"),

    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: defineRequest(client, "workforce.truncate"),

    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: defineRequest(client, "workforce.respond"),

    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: defineRequest(client, "workforce.suspend"),
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
