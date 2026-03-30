import {
  type AppSchema,
  type ReqName,
  type ReqPayload,
  type ResType,
  type StrName,
  type StrPayload,
  type StrSubscription,
} from "./schema.ts"
import { type IpcTransport } from "./transport.ts"

/** Normalizes shorthand and object stream definitions into payload and optional subscription schemas. */
function getStreamSchemas<S extends AppSchema, K extends StrName<S>>(schema: S, name: K) {
  const definition = schema.server.streams[name]
  if ("safeParse" in definition) {
    return {
      payload: definition,
      subscription: undefined,
    }
  }

  return {
    payload: definition.payload,
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
  async function send<K extends ReqName<S>>(
    name: K,
    payload: ReqPayload<S, K>,
  ): Promise<ResType<S, K>> {
    const validPayload = schema.client.requests[name].payload.parse(payload)
    return (await transport.send(name, validPayload)) as ResType<S, K>
  }

  async function subscribe<K extends StrName<S>>(
    name: K,
    onMessage: (payload: StrPayload<S, K>) => void,
  ): Promise<() => void>
  async function subscribe<K extends StrName<S>>(
    name: K,
    subscription: StrSubscription<S, K>,
    onMessage: (payload: StrPayload<S, K>) => void,
  ): Promise<() => void>
  async function subscribe<K extends StrName<S>>(
    name: K,
    subscriptionOrHandler: StrSubscription<S, K> | ((payload: StrPayload<S, K>) => void),
    maybeHandler?: (payload: StrPayload<S, K>) => void,
  ): Promise<() => void> {
    if (!Object.hasOwn(schema.server.streams, name)) {
      throw new Error(`Invalid stream: ${name}`)
    }

    const { payload, subscription } = getStreamSchemas(schema, name)
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
        const validPayload = payload.parse(streamPayload) as StrPayload<S, K>
        onMessage(validPayload)
      }),
    )
  }

  return {
    send,
    subscribe,
  }
}
