/** Internal daemon plugin support contracts for statically composed feature packages. */
import {
  type Handlers,
  type InferStreamPayload,
  type IpcSchema,
  type ValidStreamName,
} from "@goddard-ai/ipc"
import type { KindRegistry, Kindstore } from "kindstore"
import type { z } from "zod"

import type { UnionToIntersection } from "./type-utils.ts"

/** Named feature extensions exposed by one daemon plugin to plugins that consume it. */
export type FeatureExtensions = Record<string, unknown>

/** Persisted JSON config scopes supported by daemon-owned root config files. */
export type ConfigScope = "user" | "project"

/** Feature-owned config fragment metadata consumed by daemon-owned config substrate. */
export type ConfigDefinition<TRawConfig = unknown, TResolvedConfig = TRawConfig> = {
  readonly schema: z.ZodType<TRawConfig>
  readonly scopes?: readonly ConfigScope[]
  readonly resolve?: (input: {
    readonly user?: TRawConfig | undefined
    readonly project?: TRawConfig | undefined
  }) => TResolvedConfig | Promise<TResolvedConfig>
}

/** Feature-owned kindstore schema fragment merged by the daemon composition root. */
export type DbSchemaDefinition = KindRegistry

/** Scoped kindstore surface available to one daemon plugin during setup. */
export type DbContext<TDb extends DbSchemaDefinition> = {
  readonly schema: TDb
  readonly batch: Kindstore<TDb, {}>["batch"]
} & {
  readonly [K in keyof TDb]: Kindstore<TDb, {}>[K]
}

type SetupResult<TPlugin> = TPlugin extends {
  readonly setup?: (...args: any[]) => infer TResult
}
  ? Awaited<TResult>
  : never

/** Extracts the first-class context fields provided by one daemon plugin. */
export type InferProvides<TPlugin> =
  Extract<SetupResult<TPlugin>, { readonly provides: FeatureExtensions }> extends {
    readonly provides: infer TProvides extends FeatureExtensions
  }
    ? TProvides
    : {}

type InferConfigValue<TPlugin> = TPlugin extends {
  readonly config: infer TConfig extends ConfigDefinition
}
  ? TConfig extends { readonly resolve: (...args: never[]) => infer TResolved }
    ? Awaited<TResolved>
    : TConfig extends ConfigDefinition<unknown, infer TResolved>
      ? TResolved
      : unknown
  : never

/** Extracts the resolved config namespace contributed by one daemon plugin. */
export type InferConfig<TPlugin> = TPlugin extends {
  readonly name: infer TName extends string
  readonly config: ConfigDefinition
}
  ? { readonly [TKey in TName]: InferConfigValue<TPlugin> }
  : {}

type InferDb<TPlugin> = TPlugin extends {
  readonly db: infer TDb extends DbSchemaDefinition
}
  ? TDb
  : {}

/** Request handler map inferred from a plugin IPC schema. */
export type RequestHandlers<TIpc> = TIpc extends IpcSchema ? Handlers<TIpc> : never

/** Runtime setup contribution shape used after plugin definitions are erased. */
export type RuntimeSetupContributions = {
  readonly requestHandlers?: Record<string, unknown>
  readonly provides?: FeatureExtensions
}

type SetupConfigContext<TConfig> = keyof TConfig extends never ? {} : { readonly config: TConfig }

type SetupDbContext<TDb> = keyof TDb extends never
  ? {}
  : { readonly db: DbContext<Extract<TDb, DbSchemaDefinition>> }

type PublishContext<TPlugin> = TPlugin extends { readonly ipc: infer TIpc extends IpcSchema }
  ? keyof TIpc["streams"] extends never
    ? {}
    : {
        readonly publish: <K extends ValidStreamName<TIpc>>(
          name: K,
          payload: InferStreamPayload<TIpc, K>,
        ) => void
      }
  : {}

/** Core daemon runtime substrate available to statically composed daemon plugins. */
export type DaemonSetupSubstrate = {
  readonly addAllowedPrToSession: (
    sessionId: `ses_${string}`,
    prNumber: number,
  ) => void | Promise<void>
  readonly backendClient: {
    readonly auth: {
      readonly startDeviceFlow: (input?: {
        readonly githubUsername?: string | undefined
      }) => Promise<{
        readonly deviceCode: string
        readonly userCode: string
        readonly verificationUri: string
        readonly expiresIn: number
        readonly interval: number
      }>
      readonly completeDeviceFlow: (input: {
        readonly deviceCode: string
        readonly githubUsername: string
      }) => Promise<{
        readonly token: string
        readonly githubUsername: string
        readonly githubUserId: number
      }>
      readonly whoami: () => Promise<{
        readonly token: string
        readonly githubUsername: string
        readonly githubUserId: number
      }>
      readonly logout: () => Promise<void>
    }
    readonly pr: {
      readonly create: (input: {
        readonly owner: string
        readonly repo: string
        readonly title: string
        readonly body?: string | undefined
        readonly head: string
        readonly base: string
      }) => Promise<{ readonly number: number; readonly url: string }>
      readonly reply: (input: {
        readonly owner: string
        readonly repo: string
        readonly prNumber: number
        readonly body: string
      }) => Promise<{ readonly success: boolean }>
    }
  }
  readonly authTokenStore: {
    readonly set: (token: string) => void | Promise<void>
    readonly delete: () => void | Promise<void>
  }
  readonly configManager: {
    readonly getRootConfig: (cwd: string) => Promise<any>
  }
  readonly getSessionByToken: (token: string) => Promise<{
    readonly sessionId: `ses_${string}`
    readonly owner: string | null
    readonly repo: string | null
    readonly allowedPrNumbers: readonly number[]
  } | null>
  readonly registryService: {
    readonly listAdapters: () => Promise<any>
  }
  readonly sessionManager: any
  readonly setRequestSessionId: (id: `ses_${string}`) => void
}

/** Infers setup context fields from a plugin's own definition and consumed plugins. */
export type SetupContext<
  TConsumes extends readonly unknown[],
  TSelf = unknown,
> = DaemonSetupSubstrate &
  PublishContext<TSelf> &
  UnionToIntersection<InferProvides<TConsumes[number]>> &
  SetupConfigContext<UnionToIntersection<InferConfig<TSelf | TConsumes[number]>>> &
  SetupDbContext<UnionToIntersection<InferDb<TSelf | TConsumes[number]>>>

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type Plugin = {
  readonly name: string
  readonly consumes?: readonly Plugin[]
  readonly config?: ConfigDefinition
  readonly db?: DbSchemaDefinition
  readonly ipc?: IpcSchema
  readonly lifecycle?: unknown
  // The erased plugin shape accepts any setup context; `definePlugin()` keeps feature authoring exact.
  readonly setup?: (
    context: any,
  ) => void | RuntimeSetupContributions | Promise<void | RuntimeSetupContributions>
  readonly register?: (...args: never[]) => void | Promise<void>
}

/** Runtime daemon feature composition produced by static composition roots. */
export type Composition = {
  readonly plugins: readonly Plugin[]
  readonly ipc: IpcSchema
  readonly config: Record<string, ConfigDefinition>
  readonly db: DbSchemaDefinition
}
