/** Internal SDK plugin support contracts for statically composed feature packages. */
import type { IpcClient, IpcSchema } from "@goddard-ai/ipc"

type SdkNamespaces = Record<string, Record<string, unknown>>

type RuntimeSdkPlugin = {
  readonly name: string
  readonly ipc: IpcSchema
  readonly create: (input: { readonly client: any }) => SdkNamespaces
}

/** SDK plugin shape used to constrain feature plugin values without widening them. */
export type SdkPluginDefinition<TIpc extends IpcSchema = IpcSchema> = {
  readonly name: string
  readonly ipc: TIpc
  readonly create: (input: { readonly client: IpcClient<TIpc> }) => SdkNamespaces
}

/** Preserves the exact SDK plugin object for composition-time type inference. */
export function defineSdkPlugin<
  const TName extends string,
  const TIpc extends IpcSchema,
  const TNamespaces extends SdkNamespaces,
>(plugin: {
  readonly name: TName
  readonly ipc: TIpc
  readonly create: (input: { readonly client: IpcClient<TIpc> }) => TNamespaces
}) {
  return plugin
}

/** Composes SDK feature plugins by merging namespace objects and rejecting method collisions. */
export function composeSdkPlugins(plugins: readonly RuntimeSdkPlugin[]) {
  return {
    create(input: Parameters<RuntimeSdkPlugin["create"]>[0]) {
      const namespaces: SdkNamespaces = {}

      for (const plugin of plugins) {
        const pluginNamespaces = plugin.create(input)

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
