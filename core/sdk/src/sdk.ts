import { actionSdkPlugin } from "@goddard-ai/action/sdk"
import { adapterSdkPlugin } from "@goddard-ai/adapter/sdk"
import { authSdkPlugin } from "@goddard-ai/auth/sdk"
import { fileSearchSdkPlugin } from "@goddard-ai/file-search/sdk"
import { inboxSdkPlugin } from "@goddard-ai/inbox/sdk"
import { loopSdkPlugin } from "@goddard-ai/loop/sdk"
import { pipelineSdkPlugin } from "@goddard-ai/pipeline/sdk"
import { pullRequestSdkPlugin } from "@goddard-ai/pull-request/sdk"
import { reviewSessionSdkPlugin } from "@goddard-ai/review-session/sdk"
import { composeSdkPlugins, type InferSdkNamespaces } from "@goddard-ai/sdk-plugin"
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
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
  adapterSdkPlugin,
  authSdkPlugin,
  fileSearchSdkPlugin,
  inboxSdkPlugin,
  loopSdkPlugin,
  pipelineSdkPlugin,
  pullRequestSdkPlugin,
  reviewSessionSdkPlugin,
  sessionSdkPlugin,
  workforceSdkPlugin,
])

type FeatureSdkNamespaces = InferSdkNamespaces<typeof sdkPlugins>

/** Constructor options for the browser-safe daemon-backed SDK facade. */
export type GoddardClientOptions = IpcClientOptions

/** Builds the health namespace with one thin method per daemon health IPC action. */
function createDaemonNamespace(client: any) {
  return {
    /** Probes daemon liveness without adding SDK-specific behavior. */
    health: async () => client.daemon.health(),
  }
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

  readonly daemon: ReturnType<typeof createDaemonNamespace>
  readonly auth: FeatureSdkNamespaces["auth"]
  readonly adapter: FeatureSdkNamespaces["adapter"]
  readonly fileSearch: FeatureSdkNamespaces["fileSearch"]
  readonly pr: FeatureSdkNamespaces["pr"]
  readonly inbox: FeatureSdkNamespaces["inbox"]
  readonly session: ReturnType<typeof createSessionNamespace>
  readonly reviewSession: FeatureSdkNamespaces["reviewSession"]
  readonly action: FeatureSdkNamespaces["action"]
  readonly loop: FeatureSdkNamespaces["loop"]
  readonly pipeline: FeatureSdkNamespaces["pipeline"]
  readonly workforce: FeatureSdkNamespaces["workforce"]

  constructor(options: GoddardClientOptions) {
    this.#client = resolveIpcClient(options)
    const features = sdkPlugins.wrap({
      client: this.#client,
    }) as FeatureSdkNamespaces

    this.daemon = createDaemonNamespace(this.#client)
    this.auth = features.auth
    this.adapter = features.adapter
    this.fileSearch = features.fileSearch
    this.pr = features.pr
    this.inbox = features.inbox
    this.session = createSessionNamespace(this.#client, features.session)
    this.reviewSession = features.reviewSession
    this.action = features.action
    this.loop = features.loop
    this.pipeline = features.pipeline
    this.workforce = features.workforce
  }
}
