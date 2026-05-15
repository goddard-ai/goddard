/** Internal SDK plugin support contracts for statically composed feature packages. */
import type {
  InferStreamFilter,
  InferStreamPayload,
  IpcClient,
  IpcSchema,
  RequestArguments,
  StreamTarget,
  ValidRequestName,
  ValidStreamName,
} from "@goddard-ai/ipc"

type SdkNamespaces = Record<string, Record<string, unknown>>

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

type FilteredSubscribeOverload<TFilter, TPayload> =
  | ((filter: TFilter, onMessage: (payload: TPayload) => void) => Promise<() => void>)
  | ((
      filter: TFilter,
      onMessage: (payload: TPayload) => void,
      onError: (error: unknown) => void,
    ) => () => void)
  | ((
      filter: TFilter,
      onMessage: (payload: TPayload) => void,
      onError?: (error: unknown) => void,
    ) => (() => void) | Promise<() => void>)

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

/** Defines one SDK method as a thin typed call to a feature-owned daemon IPC request. */
export function defineRequest<S extends IpcSchema, K extends ValidRequestName<S>>(
  client: IpcClient<S>,
  name: K,
) {
  return (...args: RequestArguments<S, K>) => client.send(name, ...args)
}

/** Defines one SDK method as a thin typed subscription to a feature-owned daemon IPC stream. */
export function defineSubscription<S extends IpcSchema, K extends ValidStreamName<S>>(
  client: IpcClient<S>,
  name: K,
) {
  type SubscribeOverload =
    | ((onMessage: (payload: InferStreamPayload<S, K>) => void) => Promise<() => void>)
    | ((
        onMessage: (payload: InferStreamPayload<S, K>) => void,
        onError: (error: unknown) => void,
      ) => () => void)
    | ((
        onMessage: (payload: InferStreamPayload<S, K>) => void,
        onError?: (error: unknown) => void,
      ) => (() => void) | Promise<() => void>)

  return function subscribe(
    filter: InferStreamFilter<S, K> | ((payload: InferStreamPayload<S, K>) => void),
    onMessage?: ((payload: InferStreamPayload<S, K>) => void) | ((error: unknown) => void),
    onError?: (error: unknown) => void,
  ) {
    if (typeof filter === "function") {
      return client.subscribe(name as StreamTarget<S, K>, filter, onMessage)
    }

    return client.subscribe(
      { name, filter } as StreamTarget<S, K>,
      onMessage as (payload: InferStreamPayload<S, K>) => void,
      onError,
    )
  } as UnionToIntersection<
    [InferStreamFilter<S, K>] extends [void]
      ? SubscribeOverload
      : SubscribeOverload extends infer TOverload
        ? TOverload extends (...args: infer TArgs) => infer TResult
          ? (filter: InferStreamFilter<S, K>, ...args: TArgs) => TResult
          : never
        : never
  >
}

/** Defines one SDK subscription that exposes a product-facing value from a daemon stream envelope. */
export function defineUnwrappedSubscription<
  S extends IpcSchema,
  K extends ValidStreamName<S>,
  TPayload,
>(client: IpcClient<S>, name: K, unwrap: (payload: InferStreamPayload<S, K>) => TPayload) {
  return function subscribe(
    filter: InferStreamFilter<S, K>,
    onMessage: (payload: TPayload) => void,
    onError?: (error: unknown) => void,
  ) {
    return client.subscribe(
      { name, filter } as StreamTarget<S, K>,
      (payload) => {
        onMessage(unwrap(payload))
      },
      onError,
    )
  } as UnionToIntersection<FilteredSubscribeOverload<InferStreamFilter<S, K>, TPayload>>
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
