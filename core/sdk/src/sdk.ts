import type * as acp from "@agentclientprotocol/sdk"
import { actionSdkPlugin } from "@goddard-ai/action/sdk"
import { adapterSdkPlugin } from "@goddard-ai/adapter/sdk"
import { authSdkPlugin } from "@goddard-ai/auth/sdk"
import { inboxSdkPlugin } from "@goddard-ai/inbox/sdk"
import { loopSdkPlugin } from "@goddard-ai/loop/sdk"
import { pullRequestSdkPlugin } from "@goddard-ai/pull-request/sdk"
import type {
  CancelSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest,
  GetSessionHistoryRequest,
  ListSessionsRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  SessionSubpackagesRequest,
  SteerSessionRequest,
} from "@goddard-ai/schema/daemon"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/id"
import { composeSdkPlugins, type InferSdkNamespaces } from "@goddard-ai/sdk-plugin"
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

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
  actionSdkPlugin,
  adapterSdkPlugin,
  authSdkPlugin,
  inboxSdkPlugin,
  loopSdkPlugin,
  pullRequestSdkPlugin,
  workforceSdkPlugin,
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
    return defineCachedNamespace(this, "action", this.#features.action)
  }

  get loop() {
    return defineCachedNamespace(this, "loop", this.#features.loop)
  }

  get workforce() {
    return defineCachedNamespace(this, "workforce", this.#features.workforce)
  }
}
