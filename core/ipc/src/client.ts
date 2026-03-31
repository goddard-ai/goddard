import {
  type AppSchema,
  type InferRequestPayload,
  type InferResponseType,
  type InferStreamPayload,
  type InferStreamSubscription,
  type RequestArguments,
  type ValidRequestName,
  type ValidStreamName,
} from "./schema.ts"
import { type IpcTransport } from "./transport.ts"

/** Normalizes shorthand and object stream definitions into optional subscription schemas. */
function getStreamSchemas<S extends AppSchema, K extends ValidStreamName<S>>(schema: S, name: K) {
  const definition = schema.streams[name]
  if (!("payload" in definition)) {
    return {
      subscription: undefined,
    }
  }

  return {
    subscription: definition.subscription,
  }
}

/**
 * Creates an IPC client for the given application schema and transport.
 *
 * @param schema - The IPC application schema defining the valid requests and streams.
 * @param transport - The transport layer to use for sending requests and subscribing to streams.
 * @returns An object with strongly-typed `send` and `subscribe` methods.
 */
export function createClient<S extends AppSchema>(schema: S, transport: IpcTransport) {
  async function send<K extends ValidRequestName<S>>(
    name: K,
    ...args: RequestArguments<S, K>
  ): Promise<InferResponseType<S, K>> {
    const definition = schema.requests[name]
    if (!("payload" in definition)) {
      return (await transport.send(name, undefined)) as InferResponseType<S, K>
    }

    const validPayload = definition.payload.parse(args[0] as InferRequestPayload<S, K>)
    return (await transport.send(name, validPayload)) as InferResponseType<S, K>
  }

  async function subscribe<K extends ValidStreamName<S>>(
    name: K,
    onMessage: (payload: InferStreamPayload<S, K>) => void,
  ): Promise<() => void>
  async function subscribe<K extends ValidStreamName<S>>(
    name: K,
    subscription: InferStreamSubscription<S, K>,
    onMessage: (payload: InferStreamPayload<S, K>) => void,
  ): Promise<() => void>
  async function subscribe<K extends ValidStreamName<S>>(
    name: K,
    subscriptionOrHandler:
      | InferStreamSubscription<S, K>
      | ((payload: InferStreamPayload<S, K>) => void),
    maybeHandler?: (payload: InferStreamPayload<S, K>) => void,
  ): Promise<() => void> {
    if (!Object.hasOwn(schema.streams, name)) {
      throw new Error(`Invalid stream: ${name}`)
    }

    const { subscription } = getStreamSchemas(schema, name)
    const onMessage =
      typeof subscriptionOrHandler === "function" ? subscriptionOrHandler : maybeHandler

    if (!onMessage) {
      throw new Error(`Missing stream handler for ${name}`)
    }

    if (typeof subscriptionOrHandler !== "function" && !subscription) {
      throw new Error(`Stream ${name} does not accept subscription params`)
    }

    const validSubscription =
      typeof subscriptionOrHandler === "function"
        ? undefined
        : subscription?.parse(subscriptionOrHandler)

    return await Promise.resolve(
      transport.subscribe(name, validSubscription, (streamPayload) => {
        onMessage(streamPayload as InferStreamPayload<S, K>)
      }),
    )
  }

  return {
    send,
    subscribe,
  }
}
