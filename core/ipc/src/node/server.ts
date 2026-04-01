import { existsSync, unlinkSync } from "node:fs"
import * as http from "node:http"
import {
  type InferStreamPayload,
  type InferStreamSubscription,
  type IpcSchema,
  type ValidRequestName,
  type ValidStreamName,
} from "../schema.ts"
import { type Handlers } from "../types.ts"

/** Converts one unknown thrown value into a stable IPC error string. */
function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** Writes one JSON response to the socket-backed HTTP response. */
function sendJson(res: http.ServerResponse, statusCode: number, body: unknown): void {
  res.writeHead(statusCode, { "Content-Type": "application/json" })
  res.end(JSON.stringify(body))
}

/** Reads one IPC request body into a UTF-8 string payload. */
async function readBody(req: http.IncomingMessage): Promise<string> {
  let body = ""
  for await (const chunk of req) {
    body += chunk
  }
  return body
}

/** Removes one stale socket path while tolerating races with other cleanup. */
function safeUnlink(socketPath: string): void {
  if (!existsSync(socketPath)) {
    return
  }

  try {
    unlinkSync(socketPath)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      throw error
    }
  }
}

/** Normalizes shorthand and object stream definitions into optional subscription schemas. */
function getStreamSchemas<TSchema extends IpcSchema, K extends ValidStreamName<TSchema>>(
  schema: TSchema,
  name: K,
) {
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

/** Matches one published stream payload against one validated subscription filter. */
function matchesSubscriptionFilter(subscription: unknown, payload: unknown): boolean {
  if (subscription === undefined) {
    return true
  }

  if (
    typeof subscription !== "object" ||
    subscription === null ||
    typeof payload !== "object" ||
    payload === null
  ) {
    return Object.is(subscription, payload)
  }

  return Object.entries(subscription).every(([key, value]) =>
    Object.is((payload as Record<string, unknown>)[key], value),
  )
}

/** Optional hooks that run when one client subscribes to a server-published stream. */
type SubscribeHookInput<TSchema extends IpcSchema> = {
  name: ValidStreamName<TSchema>
  subscription: InferStreamSubscription<TSchema, ValidStreamName<TSchema>> | undefined
}

/** Request payload made available to per-request context factories. */
type CreateRequestContextInput<TSchema extends IpcSchema> = {
  name: ValidRequestName<TSchema>
  payload: unknown
}

/** Lifecycle data passed to request-received hooks. */
type RequestReceivedHookInput<TSchema extends IpcSchema, TContext> = {
  name: ValidRequestName<TSchema>
  payload: unknown
  context: TContext
}

/** Lifecycle data passed to request-response hooks. */
type ResponseSentHookInput<TSchema extends IpcSchema, TContext> = {
  name: ValidRequestName<TSchema>
  payload: unknown
  response: unknown
  context: TContext
  durationMs: number
}

/** Lifecycle data passed to request-failed hooks. */
type RequestFailedHookInput<TSchema extends IpcSchema, TContext> = {
  name: ValidRequestName<TSchema>
  payload: unknown
  error: unknown
  context: TContext
  durationMs: number
}

/** Optional hooks that run during request and subscription handling. */
type CreateServerOptions<TSchema extends IpcSchema, TContext> = {
  createRequestContext?: (input: CreateRequestContextInput<TSchema>) => TContext
  onRequestReceived?: (input: RequestReceivedHookInput<TSchema, TContext>) => Promise<void> | void
  onResponseSent?: (input: ResponseSentHookInput<TSchema, TContext>) => Promise<void> | void
  onRequestFailed?: (input: RequestFailedHookInput<TSchema, TContext>) => Promise<void> | void
  onSubscribe?: (input: SubscribeHookInput<TSchema>) => Promise<void> | void
}

/** Creates the Node IPC server for one socket-backed application schema. */
export function createServer<TSchema extends IpcSchema, TContext = undefined>(
  socketPath: string,
  schema: TSchema,
  handlers: Handlers<TSchema, TContext>,
  options: CreateServerOptions<TSchema, TContext> = {},
) {
  const streamClients = new Set<{
    name: string
    subscription: unknown
    res: http.ServerResponse
  }>()

  function publish<K extends ValidStreamName<TSchema>>(
    name: K,
    payload: InferStreamPayload<TSchema, K>,
  ) {
    const chunk = JSON.stringify({ name, payload }) + "\n"

    for (const client of streamClients) {
      if (
        client.name === name &&
        matchesSubscriptionFilter(client.subscription, payload) &&
        !client.res.destroyed &&
        !client.res.writableEnded
      ) {
        client.res.write(chunk)
      }
    }
  }

  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost")

    if (req.method === "POST" && url.pathname === "/") {
      void (async () => {
        const startedAt = Date.now()
        let requestName: ValidRequestName<TSchema> | undefined
        let payload: unknown
        let context: TContext | undefined

        try {
          const body = await readBody(req)
          const message = JSON.parse(body) as { name?: unknown; payload?: unknown }

          if (typeof message.name !== "string") {
            throw new Error("Request name must be a string")
          }

          requestName = message.name as ValidRequestName<TSchema>
          const routeDef = schema.requests[requestName]
          if (!routeDef) {
            throw new Error(`Unknown request: ${requestName}`)
          }

          if (routeDef.payload) {
            payload = routeDef.payload.parse(message.payload)
          }

          context = options.createRequestContext?.({
            name: requestName,
            payload,
          })

          if (context)
            await options.onRequestReceived?.({
              name: requestName,
              payload,
              context,
            })

          const handler: (...args: any[]) => any = handlers[requestName]
          const responseData = routeDef.payload
            ? await handler(payload, context)
            : await handler(context)

          if (context)
            await options.onResponseSent?.({
              name: requestName,
              payload,
              response: responseData,
              context,
              durationMs: Date.now() - startedAt,
            })

          sendJson(res, 200, responseData)
        } catch (error) {
          if (context && requestName)
            await options.onRequestFailed?.({
              name: requestName,
              payload,
              error,
              context,
              durationMs: Date.now() - startedAt,
            })

          sendJson(res, 400, { error: getErrorMessage(error) })
        }
      })()
      return
    }

    if (req.method === "GET" && url.pathname === "/stream") {
      void (async () => {
        const name = url.searchParams.get("name")
        if (!name || !Object.hasOwn(schema.streams, name)) {
          res.writeHead(400, { "Content-Type": "text/plain" })
          res.end("Invalid stream name")
          return
        }

        let subscription: InferStreamSubscription<TSchema, ValidStreamName<TSchema>> | undefined
        try {
          const { subscription: subscriptionSchema } = getStreamSchemas(
            schema,
            name as ValidStreamName<TSchema>,
          )
          const rawSubscription = url.searchParams.get("subscription")
          if (rawSubscription && !subscriptionSchema) {
            throw new Error(`Stream ${name} does not accept subscription params`)
          }
          subscription =
            rawSubscription && subscriptionSchema
              ? (subscriptionSchema.parse(JSON.parse(rawSubscription)) as InferStreamSubscription<
                  TSchema,
                  ValidStreamName<TSchema>
                >)
              : undefined
        } catch (error) {
          res.writeHead(400, { "Content-Type": "text/plain" })
          res.end(getErrorMessage(error))
          return
        }

        try {
          await options.onSubscribe?.({
            name: name as ValidStreamName<TSchema>,
            subscription,
          })
        } catch (error) {
          res.writeHead(400, { "Content-Type": "text/plain" })
          res.end(getErrorMessage(error))
          return
        }

        res.writeHead(200, {
          "Content-Type": "application/x-ndjson",
          "Cache-Control": "no-cache",
          Connection: "keep-alive",
        })
        res.flushHeaders()

        const client = { name, subscription, res }
        streamClients.add(client)

        const removeClient = () => {
          streamClients.delete(client)
        }

        req.on("close", removeClient)
        res.on("close", removeClient)
      })()
      return
    }

    res.writeHead(404)
    res.end()
  })

  safeUnlink(socketPath)
  server.listen(socketPath)
  server.on("close", () => {
    safeUnlink(socketPath)
  })

  return { server, publish }
}
