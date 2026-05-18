/** Internal daemon plugin support contracts for statically composed feature packages. */
import {
  composeIpcSchemas,
  type Handlers,
  type InferStreamPayload,
  type IpcSchema,
  type ValidStreamName,
} from "@goddard-ai/ipc"
import type { KindRegistry, Kindstore } from "kindstore"
import type { z } from "zod"

/** Named feature extensions exposed by one daemon plugin to plugins that consume it. */
export type FeatureExtensions = Record<string, unknown>

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

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

type RequestHandlers<TIpc> = TIpc extends IpcSchema ? Handlers<TIpc> : never

type RuntimeSetupContributions = {
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

type RegisterFunction = (...args: never[]) => void | Promise<void>

type ConsumedPlugins<TConsumes> =
  Extract<TConsumes, readonly Plugin[]> extends never ? [] : Extract<TConsumes, readonly Plugin[]>

type OptionalPluginField<TKey extends string, TValue> = undefined extends TValue
  ? {}
  : { readonly [K in TKey]: TValue }

type PluginShape<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinition | undefined,
  TDb extends DbSchemaDefinition | undefined,
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = { readonly name: TName } & OptionalPluginField<"consumes", TConsumes> &
  OptionalPluginField<"config", TConfig> &
  OptionalPluginField<"db", TDb> &
  OptionalPluginField<"ipc", TIpc> &
  OptionalPluginField<"lifecycle", TLifecycle> &
  OptionalPluginField<"register", TRegister>

type PluginOptions<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinition | undefined,
  TDb extends DbSchemaDefinition | undefined,
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = {
  readonly name: TName
  readonly consumes?: TConsumes
  readonly provides?: never
  readonly config?: TConfig
  readonly db?: TDb
  readonly ipc?: TIpc
  readonly lifecycle?: TLifecycle
  readonly register?: TRegister
}

type PluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) =>
  | void
  | SetupContributions<TIpc, TProvides>
  | Promise<void | SetupContributions<TIpc, TProvides>>

type RequiredPluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) => SetupContributions<TIpc, TProvides> | Promise<SetupContributions<TIpc, TProvides>>

type RequestHandlerContributions<TIpc> = TIpc extends IpcSchema
  ? {
      readonly requestHandlers: RequestHandlers<TIpc>
    }
  : {
      readonly requestHandlers?: never
    }

type ProvidesContribution<TProvides> = TProvides extends FeatureExtensions
  ? {
      readonly provides: TProvides
    }
  : {
      readonly provides?: never
    }

type SetupContributions<
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = RequestHandlerContributions<TIpc> & ProvidesContribution<TProvides>

type DefinePlugin = {
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TIpc extends IpcSchema,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister> & {
      readonly ipc: TIpc
      readonly setup: RequiredPluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister>,
        TIpc,
        TProvides
      >
    },
  ): PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister> & {
    readonly setup: RequiredPluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister>,
      TIpc,
      TProvides
    >
  }
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister> & {
      readonly ipc?: undefined
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister>,
        undefined,
        TProvides
      >
    },
  ): PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister> & {
    readonly setup?: PluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister>,
      undefined,
      TProvides
    >
  }
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export const definePlugin = ((plugin: Plugin) => plugin) as DefinePlugin

/** Composes statically imported daemon feature plugins and validates dependency ownership. */
export function composePlugins(plugins: readonly Plugin[]) {
  assertUniquePluginNames(plugins)
  assertConsumedPluginsAreComposed(plugins)
  const orderedPlugins = sortPluginsByDependency(plugins)

  const config: Record<string, ConfigDefinition> = {}
  const db: DbSchemaDefinition = {}
  const ipcSchemas: IpcSchema[] = []

  for (const plugin of orderedPlugins) {
    if (plugin.config) {
      config[plugin.name] = plugin.config
    }
    if (plugin.ipc) {
      ipcSchemas.push(plugin.ipc)
    }
    for (const [key, kind] of Object.entries(plugin.db ?? {})) {
      if (db[key]) {
        throw new Error(`Duplicate daemon plugin DB collection: ${key}`)
      }
      db[key] = kind
    }
  }

  return {
    plugins: orderedPlugins,
    ipc: composeIpcSchemas(ipcSchemas),
    config,
    db,
  } satisfies Composition
}

function assertUniquePluginNames(plugins: readonly Plugin[]) {
  const names = new Set<string>()

  for (const plugin of plugins) {
    if (names.has(plugin.name)) {
      throw new Error(`Duplicate daemon plugin: ${plugin.name}`)
    }
    names.add(plugin.name)
  }
}

function assertConsumedPluginsAreComposed(plugins: readonly Plugin[]) {
  const pluginNames = new Set(plugins.map((plugin) => plugin.name))

  for (const plugin of plugins) {
    for (const consumedPlugin of plugin.consumes ?? []) {
      if (!pluginNames.has(consumedPlugin.name)) {
        throw new Error(
          `Daemon plugin ${plugin.name} consumes ${consumedPlugin.name}, but ${consumedPlugin.name} is not composed.`,
        )
      }
    }
  }
}

function sortPluginsByDependency(plugins: readonly Plugin[]) {
  const orderedPlugins: Plugin[] = []
  const visiting = new Set<string>()
  const visited = new Set<string>()
  const pluginsByName = new Map(plugins.map((plugin) => [plugin.name, plugin]))

  function visit(plugin: Plugin, path: readonly string[]) {
    if (visited.has(plugin.name)) {
      return
    }

    if (visiting.has(plugin.name)) {
      throw new Error(`Circular daemon plugin dependency: ${[...path, plugin.name].join(" -> ")}`)
    }

    visiting.add(plugin.name)

    for (const consumedPlugin of plugin.consumes ?? []) {
      visit(pluginsByName.get(consumedPlugin.name) ?? consumedPlugin, [...path, plugin.name])
    }

    visiting.delete(plugin.name)
    visited.add(plugin.name)
    orderedPlugins.push(plugin)
  }

  for (const plugin of plugins) {
    visit(plugin, [])
  }

  return orderedPlugins
}
