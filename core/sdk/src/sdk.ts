import { actionSdkPlugin } from "@goddard-ai/action/sdk"
import { agentSdkPlugin } from "@goddard-ai/agent/sdk"
import { authSdkPlugin } from "@goddard-ai/auth/sdk"
import { fileSearchSdkPlugin } from "@goddard-ai/file-search/sdk"
import { inboxSdkPlugin } from "@goddard-ai/inbox/sdk"
import { loopSdkPlugin } from "@goddard-ai/loop/sdk"
import { pullRequestSdkPlugin } from "@goddard-ai/pull-request/sdk"
import { reviewSessionSdkPlugin } from "@goddard-ai/review-session/sdk"
import type {
  DaemonEventsStreamRequest,
  UpdateUserConfigRequest,
} from "@goddard-ai/schema/daemon-ipc"
import {
  composeSdkPlugins,
  type EventDefinition,
  type EventDefinitionOptions,
  type EventDefinitions,
  type InferSdkEvents,
  type InferSdkNamespaces,
} from "@goddard-ai/sdk-plugin"
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { taskSdkPlugin } from "@goddard-ai/task/sdk"
import { terminalSdkPlugin, type GoddardTerminalNamespace } from "@goddard-ai/terminal/sdk"
import { vscodeTaskSdkPlugin, type GoddardVscodeTaskNamespace } from "@goddard-ai/vscode-task/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"
import type * as acp from "acp-client/protocol"

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
  authSdkPlugin,
  fileSearchSdkPlugin,
  inboxSdkPlugin,
  loopSdkPlugin,
  agentSdkPlugin,
  pullRequestSdkPlugin,
  reviewSessionSdkPlugin,
  sessionSdkPlugin,
  taskSdkPlugin,
  terminalSdkPlugin,
  vscodeTaskSdkPlugin,
  workforceSdkPlugin,
])

type FeatureSdkNamespaces = InferSdkNamespaces<typeof sdkPlugins>
type FeatureSdkEvents = InferSdkEvents<typeof sdkPlugins>
type InferEventPayload<TDefinition> =
  TDefinition extends EventDefinition<infer TPayload> ? TPayload : never
type EventName<TEvents extends EventDefinitions> = keyof TEvents & string
type EventEnvelopeUnion<TEvents extends EventDefinitions> = {
  [TName in keyof TEvents & string]: {
    readonly id: string
    readonly at: string
    readonly name: TName
    readonly payload: InferEventPayload<TEvents[TName]>
    readonly log?: EventDefinitionOptions
  }
}[keyof TEvents & string]
type EventStreamRequest<TEvents extends EventDefinitions> = Omit<
  DaemonEventsStreamRequest,
  "names"
> & {
  readonly names?: readonly EventName<TEvents>[]
}
type EventStreamRequestForNames<
  TEvents extends EventDefinitions,
  TNames extends readonly EventName<TEvents>[],
> = Omit<DaemonEventsStreamRequest, "names"> & {
  readonly names: TNames
}
type EventEnvelopeForNames<
  TEvents extends EventDefinitions,
  TNames extends readonly EventName<TEvents>[],
> = EventEnvelopeUnion<Pick<TEvents, TNames[number]>>
type EventsNamespace<TEvents extends EventDefinitions> = {
  stream<const TNames extends readonly EventName<TEvents>[]>(
    input: EventStreamRequestForNames<TEvents, TNames>,
    options?: { signal?: AbortSignal },
  ): Promise<AsyncIterable<EventEnvelopeForNames<TEvents, TNames>>>
  stream(
    input?: EventStreamRequest<TEvents>,
    options?: { signal?: AbortSignal },
  ): Promise<AsyncIterable<EventEnvelopeUnion<TEvents>>>
}

export type GoddardEventEnvelope = EventEnvelopeUnion<FeatureSdkEvents>

/** Constructor options for the browser-safe daemon-backed SDK facade. */
export type GoddardClientOptions = IpcClientOptions

/** Builds the user configuration namespace with thin methods for daemon-owned persistence. */
function createConfigNamespace(client: any) {
  return {
    /** Reads the persisted user configuration and composed rendering schema. */
    get: async () => client.config.get(),
    /** Applies one validated field update to the persisted user configuration. */
    update: async (input: UpdateUserConfigRequest) => client.config.update(input),
  }
}

/** Builds the health namespace with one thin method per daemon health IPC action. */
function createDaemonNamespace(client: any) {
  return {
    /** Probes daemon liveness without adding SDK-specific behavior. */
    health: async () => client.daemon.health(),
  }
}

/** Builds the daemon event namespace with one thin method for the unified event stream. */
function createEventsNamespace<TEvents extends EventDefinitions>(
  client: any,
): EventsNamespace<TEvents> {
  return {
    /** Streams composed daemon events with optional payload-relative exact-match filters. */
    stream: (input: DaemonEventsStreamRequest = {}, options?: { signal?: AbortSignal }) =>
      client.events.stream(input, options),
  } as EventsNamespace<TEvents>
}

/** Builds the session namespace with one thin method per daemon session IPC action. */
function createSessionNamespace(client: any, sessionFeature: FeatureSdkNamespaces["session"]) {
  return {
    /** Starts or reconnects one live daemon-backed session and returns an object-backed wrapper. */
    run: async (input: SessionParams, handler?: acp.Client) => runSession(client, input, handler),

    ...sessionFeature,

    /** Sends one ACP permission response through the daemon-managed session transport. */
    respondPermission: async (input: SessionPermissionResponseRequest) =>
      client.session.send({
        id: input.id,
        message: createSessionPermissionResponseMessage(input),
      }),
    /** Reconnects if needed, then sends one prompt without exposing raw ACP message construction. */
    prompt: async (input: SessionPromptRequest) => {
      const { session } = await client.session.connect({ id: input.id })

      return client.session.send({
        id: input.id,
        message: createSessionPromptMessage({
          ...input,
          acpId: session.acpSessionId,
        }),
      })
    },
  }
}

/** Browser-safe SDK facade that mirrors the daemon IPC contract through thin namespace methods. */
export class GoddardSdk {
  readonly #client: GoddardClient

  readonly config: ReturnType<typeof createConfigNamespace>
  readonly daemon: ReturnType<typeof createDaemonNamespace>
  readonly events: EventsNamespace<FeatureSdkEvents>
  readonly auth: FeatureSdkNamespaces["auth"]
  readonly agent: FeatureSdkNamespaces["agent"]
  readonly fileSearch: FeatureSdkNamespaces["fileSearch"]
  readonly pr: FeatureSdkNamespaces["pr"]
  readonly inbox: FeatureSdkNamespaces["inbox"]
  readonly session: ReturnType<typeof createSessionNamespace>
  readonly reviewSession: FeatureSdkNamespaces["reviewSession"]
  readonly action: FeatureSdkNamespaces["action"]
  readonly loop: FeatureSdkNamespaces["loop"]
  readonly task: FeatureSdkNamespaces["task"]
  readonly terminal: GoddardTerminalNamespace
  readonly vscodeTask: GoddardVscodeTaskNamespace
  readonly workforce: FeatureSdkNamespaces["workforce"]

  constructor(options: GoddardClientOptions) {
    this.#client = resolveIpcClient(options)
    const features = sdkPlugins.wrap({
      client: this.#client,
    }) as FeatureSdkNamespaces

    this.config = createConfigNamespace(this.#client)
    this.daemon = createDaemonNamespace(this.#client)
    this.events = createEventsNamespace<FeatureSdkEvents>(this.#client)
    this.auth = features.auth
    this.agent = features.agent
    this.fileSearch = features.fileSearch
    this.pr = features.pr
    this.inbox = features.inbox
    this.session = createSessionNamespace(this.#client, features.session)
    this.reviewSession = features.reviewSession
    this.action = features.action
    this.loop = features.loop
    this.task = features.task
    this.terminal = features.terminal as GoddardTerminalNamespace
    this.vscodeTask = features.vscodeTask as GoddardVscodeTaskNamespace
    this.workforce = features.workforce
  }
}
