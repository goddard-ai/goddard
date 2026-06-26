/** Internal SDK plugin support contracts for statically composed feature packages. */
import { composeIpcRoutes, type HttpRouteTree, type RouzerClient } from "@goddard-ai/ipc"

type SdkNamespaces = Record<string, Record<string, unknown>>

export type EventDefinitionOptions = {
  readonly debug?: string
}
export type EventDefinition<TPayload = unknown> = {
  readonly payload?: TPayload
  readonly options?: EventDefinitionOptions
}
export type EventDefinitions = Record<string, EventDefinition<any>>
type HttpNode = HttpRouteTree[string]

/** Declares one daemon event payload type without adding runtime behavior. */
export function event<TPayload>(options: EventDefinitionOptions = {}): EventDefinition<TPayload> {
  return Object.keys(options).length > 0 ? { options: options } : {}
}

type RuntimeSdkPlugin = {
  readonly name: string
  readonly ipcRoutes: HttpRouteTree
  readonly events?: EventDefinitions
  readonly wrap?: (input: { readonly client: any }) => SdkNamespaces
}

type InferPluginNamespaces<TPlugin> = TPlugin extends {
  readonly ipcRoutes: infer TRoutes extends HttpRouteTree
}
  ? RouzerClient<TRoutes> &
      (TPlugin extends {
        readonly wrap?: (...args: any[]) => infer TNamespaces
      }
        ? TNamespaces
        : {})
  : {}

type UnionToIntersection<T> = (T extends unknown ? (value: T) => void : never) extends (
  value: infer TResult,
) => void
  ? TResult
  : never

type InferPluginEvents<TPlugin> = TPlugin extends {
  readonly events?: infer TEvents extends EventDefinitions
}
  ? NonNullable<TEvents>
  : {}

/** Infers the merged namespace surface returned by an SDK plugin composition. */
export type InferSdkNamespaces<TComposition> = TComposition extends {
  readonly plugins: readonly RuntimeSdkPlugin[]
}
  ? UnionToIntersection<InferPluginNamespaces<TComposition["plugins"][number]>>
  : {}

/** Infers the merged daemon event declarations owned by an SDK plugin composition. */
export type InferSdkEvents<TComposition> = TComposition extends {
  readonly plugins: readonly RuntimeSdkPlugin[]
}
  ? UnionToIntersection<InferPluginEvents<TComposition["plugins"][number]>>
  : {}

/** SDK plugin shape used to constrain feature plugin values without widening them. */
export type SdkPluginDefinition<
  TRoutes extends HttpRouteTree = HttpRouteTree,
  TNamespaces extends SdkNamespaces = SdkNamespaces,
  TEvents extends EventDefinitions = EventDefinitions,
> = {
  readonly name: string
  readonly ipcRoutes: TRoutes
  readonly events?: TEvents
  readonly wrap?: (input: { readonly client: RouzerClient<TRoutes> }) => TNamespaces
}

/** Preserves the exact SDK plugin object for composition-time type inference. */
export function defineSdkPlugin<
  const TName extends string,
  const TRoutes extends HttpRouteTree,
  const TNamespaces extends SdkNamespaces,
  const TEvents extends EventDefinitions,
>(plugin: {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly events?: TEvents
  readonly wrap?: (input: { readonly client: RouzerClient<TRoutes> }) => TNamespaces
}): {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly events?: TEvents
  readonly wrap?: (input: { readonly client: any }) => TNamespaces
} {
  return plugin as any
}

/** Composes SDK feature plugins by merging generated route namespaces and wrapper namespaces. */
export function composeSdkPlugins<const TPlugins extends readonly RuntimeSdkPlugin[]>(
  plugins: TPlugins,
) {
  return {
    plugins,
    ipcRoutes: composeIpcRoutes(plugins.map((plugin) => plugin.ipcRoutes)),
    events: composeEvents(plugins),

    wrap(input: { readonly client: any }) {
      const namespaces: SdkNamespaces = {}

      for (const plugin of plugins) {
        const pluginNamespaces = selectRouteClientNamespaces(plugin.ipcRoutes, input.client)
        mergeNamespaces(pluginNamespaces, plugin.wrap?.(input) ?? {}, { allowOverwrite: true })
        mergeNamespaces(namespaces, pluginNamespaces)
      }

      return namespaces
    },
  }
}

function composeEvents(plugins: readonly RuntimeSdkPlugin[]) {
  const events: EventDefinitions = {}

  for (const plugin of plugins) {
    for (const [name, definition] of Object.entries(plugin.events ?? {})) {
      if (Object.hasOwn(events, name)) {
        throw new Error(`Duplicate SDK event: ${name}`)
      }

      events[name] = definition
    }
  }

  return events
}

function mergeNamespaces(
  target: SdkNamespaces,
  source: SdkNamespaces,
  options: { readonly allowOverwrite?: boolean } = {},
) {
  for (const [namespaceName, namespace] of Object.entries(source)) {
    const existingNamespace = target[namespaceName]
    if (!existingNamespace) {
      target[namespaceName] = { ...namespace }
      continue
    }

    for (const methodName of Object.keys(namespace)) {
      if (!options.allowOverwrite && Object.hasOwn(existingNamespace, methodName)) {
        throw new Error(`Duplicate SDK namespace method: ${namespaceName}.${methodName}`)
      }
    }

    Object.assign(existingNamespace, namespace)
  }
}

function selectRouteClientNamespaces(routes: HttpRouteTree, client: Record<string, any>) {
  const namespaces: SdkNamespaces = {}

  for (const [key, route] of Object.entries(routes)) {
    namespaces[key] = createRouteClientNamespace(route, client[key])
  }

  return namespaces
}

function createRouteClientNamespace(route: HttpNode, client: any): any {
  if (route.kind === "action") {
    return client
  }

  const namespace: Record<string, unknown> = {}
  for (const [key, child] of Object.entries(route.children)) {
    namespace[key] = createRouteClientNamespace(child, client?.[key])
  }

  return namespace
}
