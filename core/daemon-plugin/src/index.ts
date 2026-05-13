/** Internal daemon plugin support contracts for statically composed feature packages. */
import {
  composeIpcSchemas,
  type ComposeIpcSchemas,
  type Handlers,
  type IpcSchema,
} from "@goddard-ai/ipc"
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

/** Extracts the first-class context fields provided by one daemon plugin. */
export type InferProvides<TPlugin> = TPlugin extends {
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

type InferConfigDefinition<TPlugin> = TPlugin extends {
  readonly name: infer TName extends string
  readonly config: infer TConfig extends ConfigDefinition
}
  ? { readonly [TKey in TName]: TConfig }
  : {}

type RequestHandlers<TIpc> = TIpc extends IpcSchema ? Handlers<TIpc> : never

/** Infers setup context fields from a plugin's own definition and consumed plugins. */
export type SetupContext<
  TConsumes extends readonly unknown[],
  TSelf = unknown,
> = UnionToIntersection<InferProvides<TConsumes[number]>> & {
  readonly config: UnionToIntersection<InferConfig<TSelf | TConsumes[number]>>
}

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type Plugin = {
  readonly name: string
  readonly consumes?: readonly Plugin[]
  readonly provides?: FeatureExtensions
  readonly config?: ConfigDefinition
  readonly ipc?: IpcSchema
  readonly lifecycle?: unknown
  readonly createRequestHandlers?: (...args: any[]) => unknown
  // The erased plugin shape accepts any setup context; `definePlugin()` keeps feature authoring exact.
  readonly setup?: (context: any) => unknown | Promise<unknown>
  readonly register?: (...args: never[]) => void | Promise<void>
}

type ExtractIpcs<TPlugins extends readonly unknown[]> = TPlugins extends readonly [
  infer THead,
  ...infer TTail,
]
  ? THead extends { readonly ipc: infer TIpc extends IpcSchema }
    ? readonly [TIpc, ...ExtractIpcs<TTail>]
    : ExtractIpcs<TTail>
  : readonly []

/** Infers the resolved config map produced by one daemon plugin composition. */
export type InferCompositionConfig<TPlugins extends readonly unknown[]> = UnionToIntersection<
  InferConfig<TPlugins[number]>
>

/** Infers the config contribution definitions produced by one daemon plugin composition. */
export type InferCompositionConfigDefinitions<TPlugins extends readonly unknown[]> =
  UnionToIntersection<InferConfigDefinition<TPlugins[number]>>

/** Runtime daemon feature composition produced by static composition roots. */
export type Composition<TPlugins extends readonly Plugin[]> = {
  readonly plugins: readonly Plugin[]
  readonly ipc: ComposeIpcSchemas<ExtractIpcs<TPlugins>>
  readonly config: InferCompositionConfigDefinitions<TPlugins>
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
  TProvides extends FeatureExtensions | undefined,
  TConfig extends ConfigDefinition | undefined,
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = { readonly name: TName } & OptionalPluginField<"consumes", TConsumes> &
  OptionalPluginField<"provides", TProvides> &
  OptionalPluginField<"config", TConfig> &
  OptionalPluginField<"ipc", TIpc> &
  OptionalPluginField<"lifecycle", TLifecycle> &
  OptionalPluginField<"register", TRegister>

type PluginOptions<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TProvides extends FeatureExtensions | undefined,
  TConfig extends ConfigDefinition | undefined,
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = {
  readonly name: TName
  readonly consumes?: TConsumes
  readonly provides?: TProvides
  readonly config?: TConfig
  readonly ipc?: TIpc
  readonly lifecycle?: TLifecycle
  readonly register?: TRegister
}

type PluginSetup<TConsumes extends readonly Plugin[] | undefined, TSelf> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) => unknown | Promise<unknown>

type DefinePlugin = {
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TProvides extends FeatureExtensions | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TIpc extends IpcSchema,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TRequestHandlerArgs extends readonly unknown[],
  >(
    plugin: PluginOptions<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister> & {
      readonly ipc: TIpc
      readonly createRequestHandlers: (...args: TRequestHandlerArgs) => RequestHandlers<TIpc>
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister>
      >
    },
  ): PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister> & {
    readonly createRequestHandlers: (...args: TRequestHandlerArgs) => RequestHandlers<TIpc>
    readonly setup?: PluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister>
    >
  }
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TProvides extends FeatureExtensions | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TIpc extends IpcSchema | undefined,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
  >(
    plugin: PluginOptions<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister> & {
      readonly createRequestHandlers?: undefined
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister>
      >
    },
  ): PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister> & {
    readonly setup?: PluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TProvides, TConfig, TIpc, TLifecycle, TRegister>
    >
  }
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export const definePlugin = ((plugin: Plugin) => plugin) as DefinePlugin

/** Composes statically imported daemon feature plugins and validates dependency ownership. */
export function composePlugins<const TPlugins extends readonly Plugin[]>(plugins: TPlugins) {
  assertUniquePluginNames(plugins)
  assertConsumedPluginsAreComposed(plugins)
  const orderedPlugins = sortPluginsByDependency(plugins)

  const config: Record<string, ConfigDefinition> = {}
  const ipcSchemas: IpcSchema[] = []

  for (const plugin of orderedPlugins) {
    if (plugin.config) {
      config[plugin.name] = plugin.config
    }
    if (plugin.ipc) {
      ipcSchemas.push(plugin.ipc)
    }
  }

  return {
    plugins: orderedPlugins,
    ipc: composeIpcSchemas(ipcSchemas),
    config,
  } as unknown as Composition<TPlugins>
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
