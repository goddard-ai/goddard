import {
  type InferResponseType,
  type InferStreamPayload,
  type IpcSchema,
  type RequestArguments,
  type StreamTarget,
  type ValidRequestName,
  type ValidStreamName,
} from "./schema.ts"
import { type IpcTransport } from "./transport.ts"
import { toValidationClientError } from "./validation.ts"

/** Normalizes shorthand and object stream definitions into optional filter schemas. */
function getStreamSchemas<S extends IpcSchema, K extends ValidStreamName<S>>(schema: S, name: K) {
  const definition = schema.streams[name]
  if (!("payload" in definition)) {
    return {
      filter: undefined,
    }
  }

  return {
    filter: definition.filter,
  }
}

/** Normalizes one stream target into a stream name plus optional filter payload. */
function normalizeStreamTarget<S extends IpcSchema>(
  target: StreamTarget<S, ValidStreamName<S>>,
): { name: ValidStreamName<S>; filter: unknown } {
  if (typeof target === "string") {
    return {
      name: target,
      filter: undefined,
    }
  }

  if (!Object.hasOwn(target, "filter")) {
    throw new Error(`Stream target object for ${target.name} must include a filter`)
  }

  return {
    name: target.name,
    filter: target.filter,
  }
}

/**
 * Creates an IPC client for the given application schema and transport.
 *
 * @param schema - The IPC application schema defining the valid requests and streams.
 * @param transport - The transport layer to use for sending requests and subscribing to streams.
 * @returns An object with strongly-typed `send` and `subscribe` methods.
 */
export function createClient<S extends IpcSchema>(schema: S, transport: IpcTransport) {
  async function send<K extends ValidRequestName<S>>(
    name: K,
    ...args: RequestArguments<S, K>
  ): Promise<InferResponseType<S, K>> {
    const definition = schema.requests[name]
    let validPayload
    try {
      validPayload = definition.payload?.parse(args[0])
    } catch (error) {
      throw toValidationClientError(error, {
        schema: definition.payload,
        fallbackMessage: "Request payload is invalid",
      })
    }

    return await transport.send(name, validPayload)
  }

  async function subscribe<K extends ValidStreamName<S>>(
    target: StreamTarget<S, K>,
    onMessage: (payload: InferStreamPayload<S, K>) => void,
  ): Promise<() => void> {
    const { name, filter } = normalizeStreamTarget(target)

    if (!Object.hasOwn(schema.streams, name)) {
      throw new Error(`Invalid stream: ${name}`)
    }

    const { filter: filterSchema } = getStreamSchemas(schema, name)
    if (filter !== undefined && !filterSchema) {
      throw new Error(`Stream ${name} does not accept filter params`)
    }

    let validFilter
    try {
      validFilter = filter !== undefined ? filterSchema?.parse(filter) : undefined
    } catch (error) {
      throw toValidationClientError(error, {
        schema: filterSchema,
        fallbackMessage: `Stream ${name} filter is invalid`,
      })
    }

    return await Promise.resolve(
      transport.subscribe(name, validFilter, (streamPayload) => {
        onMessage(streamPayload as InferStreamPayload<S, K>)
      }),
    )
  }

  return {
    send,
    subscribe,
  }
}
