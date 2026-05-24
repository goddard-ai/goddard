/** Internal SDK plugin support contracts for statically composed feature packages. */
import { composeIpcRoutes, type HttpRouteTree, type RouzerClient } from "@goddard-ai/ipc"

type SdkNamespaces = Record<string, Record<string, unknown>>

type RuntimeSdkPlugin = {
  readonly name: string
  readonly ipcRoutes: HttpRouteTree
  readonly wrap?: (input: { readonly client: any }) => SdkNamespaces
}

type InferPluginNamespaces<TPlugin> = TPlugin extends {
  readonly wrap?: (...args: any[]) => infer TNamespaces
}
  ? TNamespaces
  : {}

type UnionToIntersection<T> = (T extends unknown ? (value: T) => void : never) extends (
  value: infer TResult,
) => void
  ? TResult
  : never

/** Infers the merged namespace surface returned by an SDK plugin composition. */
export type InferSdkNamespaces<TComposition> = TComposition extends {
  readonly plugins: readonly RuntimeSdkPlugin[]
}
  ? UnionToIntersection<InferPluginNamespaces<TComposition["plugins"][number]>>
  : {}

/** SDK plugin shape used to constrain feature plugin values without widening them. */
export type SdkPluginDefinition<
  TRoutes extends HttpRouteTree = HttpRouteTree,
  TNamespaces extends SdkNamespaces = SdkNamespaces,
> = {
  readonly name: string
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: { readonly client: RouzerClient<TRoutes> }) => TNamespaces
}

/** Preserves the exact SDK plugin object for composition-time type inference. */
export function defineSdkPlugin<
  const TName extends string,
  const TRoutes extends HttpRouteTree,
  const TNamespaces extends SdkNamespaces,
>(plugin: {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: { readonly client: RouzerClient<TRoutes> }) => TNamespaces
}): {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: { readonly client: any }) => TNamespaces
} {
  return plugin as any
}

/** Composes SDK feature plugins by merging route trees and wrapper namespaces. */
export function composeSdkPlugins<const TPlugins extends readonly RuntimeSdkPlugin[]>(
  plugins: TPlugins,
) {
  return {
    plugins,
    ipcRoutes: composeIpcRoutes(plugins.map((plugin) => plugin.ipcRoutes)),

    wrap(input: { readonly client: any }) {
      const namespaces: SdkNamespaces = {}

      for (const plugin of plugins) {
        const pluginNamespaces = plugin.wrap?.(input)
        if (!pluginNamespaces) {
          continue
        }

        for (const [namespaceName, namespace] of Object.entries(pluginNamespaces)) {
          const existingNamespace = namespaces[namespaceName]
          if (!existingNamespace) {
            namespaces[namespaceName] = { ...namespace }
            continue
          }

          for (const methodName of Object.keys(namespace)) {
            if (Object.hasOwn(existingNamespace, methodName)) {
              throw new Error(`Duplicate SDK namespace method: ${namespaceName}.${methodName}`)
            }
          }

          Object.assign(existingNamespace, namespace)
        }
      }

      return namespaces
    },
  }
}
