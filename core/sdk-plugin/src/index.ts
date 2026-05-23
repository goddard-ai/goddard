/** Internal SDK plugin support contracts for statically composed feature packages. */
import { composeIpcRoutes, type HttpRouteTree, type RouzerClient } from "@goddard-ai/ipc"

type SdkNamespaces = Record<string, Record<string, unknown>>
type LegacyDaemonClient = {
  readonly send: (...args: any[]) => Promise<any>
  readonly subscribe: (...args: any[]) => Promise<() => void> | (() => void)
}

type RuntimeSdkPlugin = {
  readonly name: string
  readonly ipcRoutes: HttpRouteTree
  readonly wrap?: (input: { readonly client: any }) => SdkNamespaces
}

/** SDK plugin shape used to constrain feature plugin values without widening them. */
export type SdkPluginDefinition<
  TRoutes extends HttpRouteTree = HttpRouteTree,
  TNamespaces extends SdkNamespaces = SdkNamespaces,
> = {
  readonly name: string
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: {
    readonly client: RouzerClient<TRoutes> & LegacyDaemonClient
  }) => TNamespaces
}

/** Preserves the exact SDK plugin object for composition-time type inference. */
export function defineSdkPlugin<
  const TName extends string,
  const TRoutes extends HttpRouteTree,
  const TNamespaces extends SdkNamespaces,
>(plugin: {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: {
    readonly client: RouzerClient<TRoutes> & LegacyDaemonClient
  }) => TNamespaces
}): {
  readonly name: TName
  readonly ipcRoutes: TRoutes
  readonly wrap?: (input: { readonly client: any }) => TNamespaces
} {
  return plugin as any
}

/** Composes SDK feature plugins by merging route trees and wrapper namespaces. */
export function composeSdkPlugins(plugins: readonly RuntimeSdkPlugin[]) {
  return {
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
