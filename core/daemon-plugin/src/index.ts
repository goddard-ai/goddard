/** Internal daemon plugin support contracts for statically composed feature packages. */
import { composeIpcSchemas, type ComposeIpcSchemas, type IpcSchema } from "@goddard-ai/ipc"
import type { z } from "zod"

/** Named feature extensions exposed by one daemon plugin to plugins that consume it. */
export type DaemonFeatureExtensions = Record<string, unknown>

type EmptyDaemonFeatureExtensions = Record<never, never>
type EmptyDaemonConfig = Record<never, never>
// Plugin definitions need assignable setup callbacks while `defineDaemonPlugin()` preserves
// the exact context for feature authors.
type DaemonPluginSetupFunction<TContext> = {
  bivarianceHack: (context: TContext) => unknown | Promise<unknown>
}["bivarianceHack"]

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

/** Persisted JSON config scopes supported by daemon-owned root config files. */
export type DaemonPluginConfigScope = "user" | "project"

/** Feature-owned config fragment metadata consumed by daemon-owned config substrate. */
export type DaemonPluginConfigDefinition<TRawConfig = unknown, TResolvedConfig = TRawConfig> = {
  readonly schema: z.ZodType<TRawConfig>
  readonly scopes?: readonly DaemonPluginConfigScope[]
  readonly resolve?: (input: {
    readonly user?: TRawConfig | undefined
    readonly project?: TRawConfig | undefined
  }) => TResolvedConfig | Promise<TResolvedConfig>
}

/** Extracts the first-class context fields provided by one daemon plugin. */
export type InferDaemonPluginProvides<TPlugin> = TPlugin extends {
  readonly provides: infer TProvides extends DaemonFeatureExtensions
}
  ? TProvides
  : EmptyDaemonFeatureExtensions

type InferDaemonPluginConfigValue<TPlugin> = TPlugin extends {
  readonly config: infer TConfig extends DaemonPluginConfigDefinition
}
  ? TConfig extends { readonly resolve: (...args: never[]) => infer TResolved }
    ? Awaited<TResolved>
    : TConfig extends DaemonPluginConfigDefinition<unknown, infer TResolved>
      ? TResolved
      : unknown
  : never

/** Extracts the resolved config namespace contributed by one daemon plugin. */
export type InferDaemonPluginConfig<TPlugin> = TPlugin extends {
  readonly name: infer TName extends string
  readonly config: DaemonPluginConfigDefinition
}
  ? { readonly [TKey in TName]: InferDaemonPluginConfigValue<TPlugin> }
  : EmptyDaemonConfig

type InferDaemonPluginConfigDefinition<TPlugin> = TPlugin extends {
  readonly name: infer TName extends string
  readonly config: infer TConfig extends DaemonPluginConfigDefinition
}
  ? { readonly [TKey in TName]: TConfig }
  : EmptyDaemonConfig

/** Infers setup context fields from a plugin's own definition and consumed plugins. */
export type DaemonPluginSetupContext<
  TConsumes extends readonly unknown[],
  TSelf = unknown,
> = UnionToIntersection<InferDaemonPluginProvides<TConsumes[number]>> & {
  readonly config: UnionToIntersection<InferDaemonPluginConfig<TSelf | TConsumes[number]>>
}

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type DaemonPluginDefinition = {
  readonly name: string
  readonly consumes?: readonly DaemonPluginDefinition[]
  readonly provides?: DaemonFeatureExtensions
  readonly config?: DaemonPluginConfigDefinition
  readonly ipc?: IpcSchema
  readonly lifecycle?: unknown
  readonly setup?: DaemonPluginSetupFunction<
    DaemonPluginSetupContext<readonly DaemonPluginDefinition[], DaemonPluginDefinition>
  >
  readonly register?: (...args: never[]) => void | Promise<void>
}

type ExtractDaemonPluginIpcs<TPlugins extends readonly unknown[]> = TPlugins extends readonly [
  infer THead,
  ...infer TTail,
]
  ? THead extends { readonly ipc: infer TIpc extends IpcSchema }
    ? readonly [TIpc, ...ExtractDaemonPluginIpcs<TTail>]
    : ExtractDaemonPluginIpcs<TTail>
  : readonly []

/** Infers the resolved config map produced by one daemon plugin composition. */
export type InferDaemonPluginCompositionConfig<TPlugins extends readonly unknown[]> =
  UnionToIntersection<InferDaemonPluginConfig<TPlugins[number]>>

/** Infers the config contribution definitions produced by one daemon plugin composition. */
export type InferDaemonPluginCompositionConfigDefinitions<TPlugins extends readonly unknown[]> =
  UnionToIntersection<InferDaemonPluginConfigDefinition<TPlugins[number]>>

/** Runtime daemon feature composition produced by static composition roots. */
export type DaemonPluginComposition<TPlugins extends readonly DaemonPluginDefinition[]> = {
  readonly plugins: readonly DaemonPluginDefinition[]
  readonly ipc: ComposeIpcSchemas<ExtractDaemonPluginIpcs<TPlugins>>
  readonly config: InferDaemonPluginCompositionConfigDefinitions<TPlugins>
}

type InferDaemonPluginConsumes<TPlugin> = TPlugin extends {
  readonly consumes: infer TConsumes extends readonly DaemonPluginDefinition[]
}
  ? TConsumes
  : []

/** Preserves the exact daemon plugin object for composition-time type inference. */
export function defineDaemonPlugin<const TPlugin extends Omit<DaemonPluginDefinition, "setup">>(
  plugin: TPlugin & {
    readonly setup?: DaemonPluginSetupFunction<
      DaemonPluginSetupContext<InferDaemonPluginConsumes<TPlugin>, TPlugin>
    >
  },
) {
  return plugin
}

/** Composes statically imported daemon feature plugins and validates dependency ownership. */
export function composeDaemonPlugins<const TPlugins extends readonly DaemonPluginDefinition[]>(
  plugins: TPlugins,
) {
  assertUniquePluginNames(plugins)
  assertConsumedPluginsAreComposed(plugins)
  const orderedPlugins = sortDaemonPluginsByDependency(plugins)

  const config: Record<string, DaemonPluginConfigDefinition> = {}
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
  } as unknown as DaemonPluginComposition<TPlugins>
}

function assertUniquePluginNames(plugins: readonly DaemonPluginDefinition[]) {
  const names = new Set<string>()

  for (const plugin of plugins) {
    if (names.has(plugin.name)) {
      throw new Error(`Duplicate daemon plugin: ${plugin.name}`)
    }
    names.add(plugin.name)
  }
}

function assertConsumedPluginsAreComposed(plugins: readonly DaemonPluginDefinition[]) {
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

function sortDaemonPluginsByDependency(plugins: readonly DaemonPluginDefinition[]) {
  const orderedPlugins: DaemonPluginDefinition[] = []
  const visiting = new Set<string>()
  const visited = new Set<string>()
  const pluginsByName = new Map(plugins.map((plugin) => [plugin.name, plugin]))

  function visit(plugin: DaemonPluginDefinition, path: readonly string[]) {
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
