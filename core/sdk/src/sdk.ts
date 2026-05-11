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

/** Caches a getter result on first access by replacing the instance getter with the concrete value. */
function lazy<This extends object, Value>(
  getter: (this: This) => Value,
  context: ClassGetterDecoratorContext<This, Value>,
) {
  return function getLazyValue(this: This) {
    const value = getter.call(this)

    Object.defineProperty(this, context.name, {
      configurable: true,
      value,
    })

    return value
  }
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

/** Browser-safe SDK facade that mirrors the daemon IPC contract through thin namespace methods. */
export class GoddardSdk {
  readonly #client: DaemonIpcClient

  constructor(options: GoddardClientOptions) {
    this.#client = resolveIpcClient(options)
  }

  @lazy
  get daemon() {
    return {
      /** Probes daemon liveness without adding SDK-specific behavior. */
      health: defineRequest(this.#client, "daemon.health"),
    }
  }

  @lazy
  get auth() {
    return {
      /** Starts one GitHub device flow through the daemon auth contract. */
      startDeviceFlow: defineRequest(this.#client, "auth.device.start"),

      /** Completes one pending GitHub device flow through the daemon auth contract. */
      completeDeviceFlow: defineRequest(this.#client, "auth.device.complete"),

      /** Reads the current daemon-owned auth session as-is. */
      whoami: defineRequest(this.#client, "auth.whoami"),

      /** Clears the current daemon-owned auth session as-is. */
      logout: defineRequest(this.#client, "auth.logout"),
    }
  }

  @lazy
  get adapter() {
    return {
      /** Lists adapters available for one project or global launch flow. */
      list: defineRequest(this.#client, "adapter.list"),
    }
  }

  @lazy
  get pr() {
    return {
      /** Submits one pull request through the daemon PR contract. */
      submit: defineRequest(this.#client, "pr.submit"),

      /** Fetches one daemon-managed pull request by tagged id. */
      get: defineRequest(this.#client, "pr.get"),

      /** Posts one pull request reply through the daemon PR contract. */
      reply: defineRequest(this.#client, "pr.reply"),
    }
  }

  @lazy
  get inbox() {
    return {
      /** Lists daemon-local inbox rows using daemon ordering and filtering. */
      list: defineRequest(this.#client, "inbox.list"),

      /** Updates one daemon-local inbox row by entity id. */
      update: defineRequest(this.#client, "inbox.update"),

      /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
      bulkUpdate: defineRequest(this.#client, "inbox.bulkUpdate"),

      /** Subscribes to daemon-published inbox item updates. */
      subscribe: defineSubscription(this.#client, "inbox.item"),
    }
  }

  @lazy
  get session() {
    return {
      /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
      run: defineRequest(this.#client, "session.run"),

      /** Creates one daemon-managed session record. */
      create: defineRequest(this.#client, "session.create"),

      /** Lists daemon-managed sessions and pagination state. */
      list: defineRequest(this.#client, "session.list"),

      /** Fetches one daemon-managed session record. */
      get: defineRequest(this.#client, "session.get"),

      /** Reconnects to one daemon-managed session record. */
      connect: defineRequest(this.#client, "session.connect"),

      /** Reads one daemon-managed session history with session identity and connection state. */
      history: defineRequest(this.#client, "session.history"),

      /** Reads the current git diff for one daemon-managed session workspace. */
      changes: defineRequest(this.#client, "session.changes"),

      /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
      composerSuggestions: defineRequest(this.#client, "session.composerSuggestions"),

      /** Reads draft composer suggestions that only depend on one repository cwd. */
      draftSuggestions: defineRequest(this.#client, "session.draftSuggestions"),

      /** Loads launch-time adapter and repository capabilities before a session is created. */
      launchPreview: defineRequest(this.#client, "session.launchPreview"),

      /** Discovers launchable subpackage working directories under one project cwd. */
      subpackages: defineRequest(this.#client, "session.subpackages"),

      /** Reads one daemon-managed session diagnostics with event history and connection state. */
      diagnostics: defineRequest(this.#client, "session.diagnostics"),

      /** Reads persisted worktree metadata attached to one daemon-managed session. */
      worktree: defineRequest(this.#client, "session.worktree.get"),

      /** Mounts a review session for one daemon-managed session worktree. */
      mountReviewSession: defineRequest(this.#client, "session.reviewSession.mount"),

      /** Runs one mounted review session immediately. */
      runReviewSession: defineRequest(this.#client, "session.reviewSession.run"),

      /** Unmounts a review session from one daemon-managed session worktree. */
      unmountReviewSession: defineRequest(this.#client, "session.reviewSession.unmount"),

      /** Reads persisted workforce metadata attached to one daemon-managed session. */
      workforce: defineRequest(this.#client, "session.workforce.get"),

      /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
      shutdown: defineRequest(this.#client, "session.shutdown"),

      /** Marks one session inbox row completed without shutting down the session. */
      complete: defineRequest(this.#client, "session.complete"),

      /** Records the current session initiative without creating an inbox row. */
      declareInitiative: defineRequest(this.#client, "session.declareInitiative"),

      /** Reports a session blocker and marks the session inbox row unread. */
      reportBlocker: defineRequest(this.#client, "session.reportBlocker"),

      /** Reports an end-of-turn session update when no other entity claimed attention. */
      reportTurnEnded: defineRequest(this.#client, "session.reportTurnEnded"),

      /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
      cancel: defineRequest(this.#client, "session.cancel"),

      /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
      steer: defineRequest(this.#client, "session.steer"),

      /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
      send: defineRequest(this.#client, "session.send"),

      /** Sends one ACP permission response through the daemon-managed session transport. */
      respondPermission: defineRequest(this.#client, "session.respondPermission"),

      /** Sends one prompt to a daemon-managed session without exposing raw ACP message construction. */
      prompt: defineRequest(this.#client, "session.prompt"),

      /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
      subscribe: defineSubscription(this.#client, "session.message"),

      /** Resolves one daemon session token to its daemon session id. */
      resolveToken: defineRequest(this.#client, "session.resolveToken"),
    }
  }

  @lazy
  get action() {
    return {
      /** Runs one named daemon action and creates the resulting daemon session. */
      run: defineRequest(this.#client, "action.run"),
    }
  }

  @lazy
  get loop() {
    return {
      /** Starts or reuses one daemon loop runtime. */
      start: defineRequest(this.#client, "loop.start"),

      /** Fetches one daemon loop runtime and its resolved config. */
      get: defineRequest(this.#client, "loop.get"),

      /** Lists daemon loop runtime summaries. */
      list: defineRequest(this.#client, "loop.list"),

      /** Shuts down one daemon loop and reports whether shutdown succeeded. */
      shutdown: defineRequest(this.#client, "loop.shutdown"),
    }
  }

  @lazy
  get workforce() {
    return {
      /** Starts or reuses one daemon workforce runtime. */
      start: defineRequest(this.#client, "workforce.start"),

      /** Discovers package candidates for one repository workforce initialization flow. */
      discoverCandidates: defineRequest(this.#client, "workforce.discoverCandidates"),

      /** Initializes one repository workforce config and ledger through the daemon. */
      initialize: defineRequest(this.#client, "workforce.initialize"),

      /** Fetches one daemon workforce runtime and its resolved config. */
      get: defineRequest(this.#client, "workforce.get"),

      /** Lists daemon workforce runtime summaries. */
      list: defineRequest(this.#client, "workforce.list"),

      /** Subscribes to live daemon-published workforce ledger events for one repository root. */
      subscribe: defineSubscription(this.#client, "workforce.event"),

      /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
      shutdown: defineRequest(this.#client, "workforce.shutdown"),

      /** Enqueues one workforce request and includes the updated workforce projection. */
      request: defineRequest(this.#client, "workforce.request"),

      /** Updates one workforce request and includes the updated workforce projection. */
      update: defineRequest(this.#client, "workforce.update"),

      /** Cancels one workforce request and includes the updated workforce projection. */
      cancel: defineRequest(this.#client, "workforce.cancel"),

      /** Truncates one workforce queue and includes the updated workforce projection. */
      truncate: defineRequest(this.#client, "workforce.truncate"),

      /** Responds to one active workforce request and includes the updated workforce projection. */
      respond: defineRequest(this.#client, "workforce.respond"),

      /** Suspends one active workforce request and includes the updated workforce projection. */
      suspend: defineRequest(this.#client, "workforce.suspend"),
    }
  }
}
