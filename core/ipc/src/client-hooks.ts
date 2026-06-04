/** Lifecycle events emitted by a route client around IPC route calls. */
export type IpcClientHookEvent =
  | {
      type: "request.start"
      opId: string
      routeName: string
      payload: unknown
    }
  | {
      type: "request.success"
      opId: string
      routeName: string
      response: unknown
      durationMs: number
    }
  | {
      type: "request.error"
      opId: string
      routeName: string
      error: unknown
      durationMs: number
    }

/** Observes route-client requests without changing their transport semantics. */
export type IpcClientHook = (event: IpcClientHookEvent) => void

/** Wraps a nested route client so each callable route emits request lifecycle events. */
export function createHookedIpcClient<TClient extends object>(
  client: TClient,
  hook: IpcClientHook | undefined,
): TClient {
  if (!hook) {
    return client
  }

  return createRouteNodeProxy(client, [], hook) as TClient
}

function createRouteNodeProxy(node: object, path: readonly string[], hook: IpcClientHook): unknown {
  return new Proxy(node, {
    get(target, property, receiver) {
      if (property === "then") {
        return undefined
      }

      const value = Reflect.get(target, property, receiver)
      if ((typeof value !== "object" && typeof value !== "function") || value === null) {
        return value
      }

      return createRouteNodeProxy(value, [...path, String(property)], hook)
    },
    apply(target, thisArg, argumentsList) {
      if (typeof target !== "function") {
        throw new TypeError("IPC route target is not callable")
      }

      return callHookedRoute(
        target as (...args: unknown[]) => unknown,
        thisArg,
        argumentsList,
        path.join("."),
        hook,
      )
    },
  })
}

async function callHookedRoute(
  route: (...args: unknown[]) => unknown,
  thisArg: unknown,
  argumentsList: unknown[],
  routeName: string,
  hook: IpcClientHook,
) {
  const opId = createOpId()
  const startedAt = performance.now()
  const payload = argumentsList[0]

  emitHook(hook, {
    type: "request.start",
    opId,
    routeName,
    payload,
  })

  try {
    const response = await route.apply(thisArg, argumentsList)
    emitHook(hook, {
      type: "request.success",
      opId,
      routeName,
      response,
      durationMs: performance.now() - startedAt,
    })
    return response
  } catch (error) {
    emitHook(hook, {
      type: "request.error",
      opId,
      routeName,
      error,
      durationMs: performance.now() - startedAt,
    })
    throw error
  }
}

function emitHook(hook: IpcClientHook, event: IpcClientHookEvent) {
  try {
    hook(event)
  } catch {
    // Observability hooks must not change IPC request behavior.
  }
}

function createOpId() {
  return globalThis.crypto?.randomUUID?.() ?? `ipc_${Date.now()}_${Math.random().toString(36)}`
}
