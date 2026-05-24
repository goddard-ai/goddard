import type * as acp from "@agentclientprotocol/sdk"
import { adapterSdkPlugin } from "@goddard-ai/adapter/sdk"
import { authSdkPlugin } from "@goddard-ai/auth/sdk"
import { inboxSdkPlugin } from "@goddard-ai/inbox/sdk"
import { pullRequestSdkPlugin } from "@goddard-ai/pull-request/sdk"
import type {
  CancelSessionRequest,
  CancelWorkforceRequest,
  CreateSessionRequest,
  CreateWorkforceRequest,
  DeclareSessionInitiativeRequest,
  DiscoverWorkforceCandidatesRequest,
  GetLoopRequest,
  GetSessionChangesRequest,
  GetSessionHistoryRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  ListSessionsRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  RespondWorkforceRequest,
  RunNamedActionRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  SessionSubpackagesRequest,
  ShutdownLoopRequest,
  ShutdownWorkforceRequest,
  StartLoopRequest,
  StartWorkforceRequest,
  SteerSessionRequest,
  SubscribeWorkforceEventsRequest,
  SuspendWorkforceRequest,
  TruncateWorkforceRequest,
  UpdateWorkforceRequest,
  WorkforceEventEnvelope,
} from "@goddard-ai/schema/daemon"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/id"
import { composeSdkPlugins, type InferSdkNamespaces } from "@goddard-ai/sdk-plugin"
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"

import { runSession } from "./daemon/session/client.ts"
import { resolveIpcClient, type GoddardClient, type IpcClientOptions } from "./ipc-client.ts"
import {
  createSessionPermissionResponseMessage,
  createSessionPromptMessage,
  type SessionParams,
  type SessionPermissionResponseRequest,
  type SessionPromptRequest,
} from "./session.ts"

const sdkPlugins = composeSdkPlugins([
  adapterSdkPlugin,
  authSdkPlugin,
  inboxSdkPlugin,
  pullRequestSdkPlugin,
])

type FeatureSdkNamespaces = InferSdkNamespaces<typeof sdkPlugins>

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
function createDaemonNamespace(client: any) {
  return {
    /** Probes daemon liveness without adding SDK-specific behavior. */
    health: async () => client.daemon.health(),
  }
}

/** Builds the session namespace with one thin method per daemon session IPC action. */
function createSessionNamespace(client: any) {
  const sessionFeature = sessionSdkPlugin.wrap!({ client }).session

  return {
    /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
    run: async (input: SessionParams, handler?: acp.Client) => runSession(client, input, handler),

    /** Creates one daemon-managed session record. */
    create: async (input: CreateSessionRequest) => client.session.create({ body: input }),

    /** Lists daemon-managed sessions and pagination state. */
    list: async (input: ListSessionsRequest) => client.session.list({ body: input }),

    /** Fetches one daemon-managed session record. */
    get: async (input: DaemonSessionIdParams) => client.session.get({ body: input }),

    /** Reconnects to one daemon-managed session record. */
    connect: async (input: DaemonSessionIdParams) => client.session.connect({ body: input }),

    /** Reads one daemon-managed session history with session identity and connection state. */
    history: async (input: GetSessionHistoryRequest) => client.session.history({ body: input }),

    /** Reads the current git diff for one daemon-managed session workspace. */
    changes: async (input: GetSessionChangesRequest) => client.session.changes({ body: input }),

    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: async (input: SessionComposerSuggestionsRequest) =>
      client.session.composerSuggestions({ body: input }),

    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: async (input: SessionDraftSuggestionsRequest) =>
      client.session.draftSuggestions({ body: input }),

    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: async (input: SessionLaunchPreviewRequest) =>
      client.session.launchPreview({ body: input }),

    /** Discovers launchable subpackage working directories under one project cwd. */
    subpackages: async (input: SessionSubpackagesRequest) =>
      client.session.subpackages({ body: input }),

    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: async (input: DaemonSessionIdParams) =>
      client.session.diagnostics({ body: input }),

    ...sessionFeature,

    /** Reads persisted workforce metadata attached to one daemon-managed session. */
    workforce: async (input: DaemonSessionIdParams) =>
      client.session.workforce.get({ body: input }),

    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: async (input: DaemonSessionIdParams) => client.session.shutdown({ body: input }),

    /** Marks one session inbox row completed without shutting down the session. */
    complete: async (input: DaemonSessionIdParams) => client.session.complete({ body: input }),

    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: async (input: DeclareSessionInitiativeRequest) =>
      client.session.declareInitiative({ body: input }),

    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: async (input: ReportSessionBlockerRequest) =>
      client.session.reportBlocker({ body: input }),

    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: async (input: ReportSessionTurnEndedRequest) =>
      client.session.reportTurnEnded({ body: input }),

    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: async (input: CancelSessionRequest) => client.session.cancel({ body: input }),
    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: async (input: SteerSessionRequest) => client.session.steer({ body: input }),
    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: async (input: SendSessionMessageRequest) => client.session.send({ body: input }),
    /** Sends one ACP permission response through the daemon-managed session transport. */
    respondPermission: async (input: SessionPermissionResponseRequest) =>
      client.session.send({
        body: {
          id: input.id,
          message: createSessionPermissionResponseMessage(input),
        },
      }),
    /** Sends one prompt to a daemon-managed session without exposing raw ACP message construction. */
    prompt: async (input: SessionPromptRequest) =>
      client.session.send({
        body: {
          id: input.id,
          message: createSessionPromptMessage(input),
        },
      }),
    /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
    subscribe: async (
      input: DaemonSessionIdParams,
      onMessage: (message: acp.AnyMessage) => void,
    ): Promise<() => void> => {
      const controller = new AbortController()
      const events = await client.session.messageEvents({ query: input, signal: controller.signal })
      void (async () => {
        for await (const payload of events) {
          if (controller.signal.aborted) {
            break
          }
          onMessage(payload.message)
        }
      })()
      return () => controller.abort()
    },

    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: async (input: ResolveSessionTokenRequest) =>
      client.session.resolveToken({ body: input }),
  }
}

/** Builds the action namespace with one thin method per daemon action IPC call. */
function createActionNamespace(client: any) {
  return {
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: async (input: RunNamedActionRequest) => client.action.run({ body: input }),
  }
}

/** Builds the loop namespace with one thin method per daemon loop IPC action. */
function createLoopNamespace(client: any) {
  return {
    /** Starts or reuses one daemon loop runtime. */
    start: async (input: StartLoopRequest) => client.loop.start({ body: input }),

    /** Fetches one daemon loop runtime and its resolved config. */
    get: async (input: GetLoopRequest) => client.loop.get({ body: input }),

    /** Lists daemon loop runtime summaries. */
    list: async () => client.loop.list(),

    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownLoopRequest) => client.loop.shutdown({ body: input }),
  }
}

/** Builds the workforce namespace with one thin method per daemon workforce IPC action. */
function createWorkforceNamespace(client: any) {
  return {
    /** Starts or reuses one daemon workforce runtime. */
    start: async (input: StartWorkforceRequest) => client.workforce.start({ body: input }),

    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: async (input: DiscoverWorkforceCandidatesRequest) =>
      client.workforce.discoverCandidates({ body: input }),

    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: async (input: InitializeWorkforceRequest) =>
      client.workforce.initialize({ body: input }),

    /** Fetches one daemon workforce runtime and its resolved config. */
    get: async (input: GetWorkforceRequest) => client.workforce.get({ body: input }),

    /** Lists daemon workforce runtime summaries. */
    list: async () => client.workforce.list(),

    /** Subscribes to live daemon-published workforce ledger events for one repository root. */
    subscribe: async (
      input: SubscribeWorkforceEventsRequest,
      onEvent: (event: WorkforceEventEnvelope["event"]) => void,
    ): Promise<() => void> => {
      const controller = new AbortController()
      const events = await client.workforce.event({ query: input, signal: controller.signal })
      void (async () => {
        for await (const payload of events) {
          if (controller.signal.aborted) {
            break
          }
          onEvent(payload.event)
        }
      })()
      return () => controller.abort()
    },

    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: async (input: ShutdownWorkforceRequest) => client.workforce.shutdown({ body: input }),

    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: async (input: CreateWorkforceRequest) => client.workforce.request({ body: input }),

    /** Updates one workforce request and includes the updated workforce projection. */
    update: async (input: UpdateWorkforceRequest) => client.workforce.update({ body: input }),

    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: async (input: CancelWorkforceRequest) => client.workforce.cancel({ body: input }),

    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: async (input: TruncateWorkforceRequest) => client.workforce.truncate({ body: input }),

    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: async (input: RespondWorkforceRequest) => client.workforce.respond({ body: input }),

    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: async (input: SuspendWorkforceRequest) => client.workforce.suspend({ body: input }),
  }
}

/** Browser-safe SDK facade that mirrors the daemon IPC contract through thin namespace methods. */
export class GoddardSdk {
  readonly #client: GoddardClient
  #featureNamespaces: FeatureSdkNamespaces | undefined

  constructor(options: GoddardClientOptions) {
    this.#client = resolveIpcClient(options)
  }

  get #features() {
    this.#featureNamespaces ??= sdkPlugins.wrap({
      client: this.#client,
    }) as FeatureSdkNamespaces
    return this.#featureNamespaces
  }

  get daemon() {
    return defineCachedNamespace(this, "daemon", createDaemonNamespace(this.#client))
  }

  get auth() {
    return defineCachedNamespace(this, "auth", this.#features.auth)
  }

  get adapter() {
    return defineCachedNamespace(this, "adapter", this.#features.adapter)
  }

  get pr() {
    return defineCachedNamespace(this, "pr", this.#features.pr)
  }

  get inbox() {
    return defineCachedNamespace(this, "inbox", this.#features.inbox)
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
