/** Internal daemon plugin support contracts for statically composed feature packages. */
import type { HttpRouteTree as BackendRouteTree, RouzerClient } from "@goddard-ai/backend-plugin"
import { type HttpRouteTree, type RouteRequestHandlerMap } from "@goddard-ai/ipc"
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

type InferBackendRoutes<TPlugin> = TPlugin extends {
  readonly backendRoutes: infer TBackendRoutes extends BackendRouteTree
}
  ? TBackendRoutes
  : {}

/** Route handler map inferred from a plugin Rouzer IPC route tree. */
export type RouteHandlers<TIpcRoutes> = TIpcRoutes extends HttpRouteTree
  ? RouteRequestHandlerMap<TIpcRoutes>
  : never

/** Runtime setup contribution shape used after plugin definitions are erased. */
export type RuntimeSetupContributions = {
  readonly routeHandlers?: Record<string, unknown>
  readonly provides?: FeatureExtensions
  readonly close?: () => void | Promise<void>
}

type SetupConfigContext<TConfig> = keyof TConfig extends never ? {} : { readonly config: TConfig }

type SetupDbContext<TDb> = keyof TDb extends never
  ? {}
  : { readonly db: DbContext<Extract<TDb, DbSchemaDefinition>> }

type SetupBackendContext<TBackendRoutes> = keyof TBackendRoutes extends never
  ? {}
  : { readonly backend: RouzerClient<Extract<TBackendRoutes, BackendRouteTree>> }

type PublishContext<TPlugin> = TPlugin extends { readonly ipcRoutes: HttpRouteTree }
  ? {
      readonly publish: (name: string, payload: unknown) => void
    }
  : {}

/** Core daemon runtime substrate available to statically composed daemon plugins. */
export type DaemonSetupSubstrate = {
  readonly authTokenStore: {
    readonly set: (token: string) => void | Promise<void>
    readonly delete: () => void | Promise<void>
  }
  readonly configManager: {
    readonly getRootConfig: (cwd: string) => Promise<any>
  }
  readonly registryService: {
    readonly listAdapters: () => Promise<any>
  }
  readonly sessionManager: any
  readonly getIpcRequestContext: () => {
    readonly setSessionId: (id: `ses_${string}`) => void
  }
}

/** Infers setup context fields from a plugin's own definition and consumed plugins. */
export type SetupContext<
  TConsumes extends readonly unknown[],
  TSelf = unknown,
> = DaemonSetupSubstrate &
  PublishContext<TSelf> &
  UnionToIntersection<InferProvides<TConsumes[number]>> &
  SetupConfigContext<UnionToIntersection<InferConfig<TSelf | TConsumes[number]>>> &
  SetupDbContext<UnionToIntersection<InferDb<TSelf | TConsumes[number]>>> &
  SetupBackendContext<UnionToIntersection<InferBackendRoutes<TSelf | TConsumes[number]>>>

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type Plugin = {
  readonly name: string
  readonly consumes?: readonly Plugin[]
  readonly config?: ConfigDefinition
  readonly db?: DbSchemaDefinition
  readonly backendRoutes?: BackendRouteTree
  readonly ipcRoutes?: HttpRouteTree
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
  readonly ipcRoutes: HttpRouteTree
  readonly backendRoutes: BackendRouteTree
  readonly config: Record<string, ConfigDefinition>
  readonly db: DbSchemaDefinition
}
