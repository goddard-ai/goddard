import type * as acp from "@agentclientprotocol/sdk"
import { actionSdkPlugin } from "@goddard-ai/action/sdk"
import { adapterSdkPlugin } from "@goddard-ai/adapter/sdk"
import { authSdkPlugin } from "@goddard-ai/auth/sdk"
import { inboxSdkPlugin } from "@goddard-ai/inbox/sdk"
import { loopSdkPlugin } from "@goddard-ai/loop/sdk"
import { pullRequestSdkPlugin } from "@goddard-ai/pull-request/sdk"
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
  sessionSdkPlugin,
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
    return defineCachedNamespace(
      this,
      "session",
      createSessionNamespace(this.#client, this.#features.session),
    )
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
