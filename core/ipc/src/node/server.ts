import { existsSync, unlinkSync } from "node:fs"
import * as http from "node:http"
import { z } from "zod"
import { IpcClientError } from "../errors.ts"
import {
  type InferStreamFilter,
  type InferStreamPayload,
  type IpcSchema,
  type ValidRequestName,
  type ValidStreamName,
} from "../schema.ts"
import { type Handlers } from "../types.ts"

const INTERNAL_SERVER_ERROR_MESSAGE = "Internal server error"

/** Returns the safe client-facing status code and message for one IPC server failure. */
function getErrorResponse(error: unknown): { statusCode: number; message: string } {
  return error instanceof IpcClientError
    ? { statusCode: 400, message: error.message }
    : { statusCode: 500, message: INTERNAL_SERVER_ERROR_MESSAGE }
}

/** Re-classifies validation and parse failures as client-visible IPC errors. */
function toClientError(error: unknown, fallbackMessage: string): IpcClientError {
  if (error instanceof IpcClientError) {
    return error
  }

  if (error instanceof z.ZodError) {
    return new IpcClientError(z.prettifyError(error), { cause: error })
  }

  if (error instanceof Error) {
    return new IpcClientError(fallbackMessage, { cause: error })
  }

  return new IpcClientError(fallbackMessage)
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

/** Normalizes shorthand and object stream definitions into optional filter schemas. */
function getStreamSchemas<TSchema extends IpcSchema, K extends ValidStreamName<TSchema>>(
  schema: TSchema,
  name: K,
) {
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

/** Matches one published stream payload against one validated stream filter. */
function matchesStreamFilter(filter: unknown, payload: unknown): boolean {
  if (filter === undefined) {
    return true
  }

  if (
    typeof filter !== "object" ||
    filter === null ||
    typeof payload !== "object" ||
    payload === null
  ) {
    return Object.is(filter, payload)
  }

  return Object.entries(filter).every(([key, value]) =>
    Object.is((payload as Record<string, unknown>)[key], value),
  )
}

/** Optional hooks that run when one client subscribes to a server-published stream. */
type SubscribeHookInput<TSchema extends IpcSchema> = {
  name: ValidStreamName<TSchema>
  filter: InferStreamFilter<TSchema, ValidStreamName<TSchema>> | undefined
}

/** Request metadata made available to request wrappers and lifecycle hooks. */
type RequestHookInput<TSchema extends IpcSchema> = {
  name: ValidRequestName<TSchema>
  payload: unknown
}

/** Lifecycle data passed to request-received hooks. */
type RequestReceivedHookInput<TSchema extends IpcSchema> = RequestHookInput<TSchema>

/** Lifecycle data passed to request-response hooks. */
type ResponseSentHookInput<TSchema extends IpcSchema> = RequestHookInput<TSchema> & {
  response: unknown
  durationMs: number
}

/** Lifecycle data passed to request-failed hooks. */
type RequestFailedHookInput<TSchema extends IpcSchema> = RequestHookInput<TSchema> & {
  error: unknown
  durationMs: number
}

/** Wraps one request lifecycle so callers can install ambient async context around handlers and hooks. */
type RunHandlerHook<TSchema extends IpcSchema> = <T>(
  input: RequestHookInput<TSchema>,
  handler: () => Promise<T> | T,
) => Promise<T> | T

/** Optional hooks that run during request and stream-filter handling. */
type CreateServerConfig<TSchema extends IpcSchema> = {
  socketPath: string
  schema: TSchema
  handlers: Handlers<TSchema>
  runHandler?: RunHandlerHook<TSchema>
  onRequestReceived?: (input: RequestReceivedHookInput<TSchema>) => Promise<void> | void
  onResponseSent?: (input: ResponseSentHookInput<TSchema>) => Promise<void> | void
  onRequestFailed?: (input: RequestFailedHookInput<TSchema>) => Promise<void> | void
  onSubscribe?: (input: SubscribeHookInput<TSchema>) => Promise<void> | void
}

/** Creates the Node IPC server for one socket-backed application schema. */
export function createServer<TSchema extends IpcSchema>(config: CreateServerConfig<TSchema>) {
  const {
    socketPath,
    schema,
    handlers,
    runHandler,
    onRequestReceived,
    onResponseSent,
    onRequestFailed,
    onSubscribe,
  } = config
  const streamClients = new Set<{
    name: string
    filter: unknown
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
        matchesStreamFilter(client.filter, payload) &&
        !client.res.destroyed &&
        !client.res.writableEnded
      ) {
        client.res.write(chunk)
      }
    }
  }

  async function handleRequest(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const startedAt = Date.now()
    let requestName: ValidRequestName<TSchema> | undefined
    let payload: unknown

    try {
      const body = await readBody(req)
      let message: { name?: unknown; payload?: unknown }
      try {
        message = JSON.parse(body) as { name?: unknown; payload?: unknown }
      } catch (error) {
        throw toClientError(error, "Request body must be valid JSON")
      }

      if (typeof message.name !== "string") {
        throw new IpcClientError("Request name must be a string")
      }

      requestName = message.name as ValidRequestName<TSchema>
      const routeDef = schema.requests[requestName]
      if (!routeDef) {
        throw new IpcClientError(`Unknown request: ${requestName}`)
      }

      if (routeDef.payload) {
        try {
          payload = routeDef.payload.parse(message.payload)
        } catch (error) {
          throw toClientError(error, "Request payload is invalid")
        }
      }

      const requestInput: RequestHookInput<TSchema> = {
        name: requestName,
        payload,
      }
      const processRequest = async () => {
        try {
          await onRequestReceived?.(requestInput)

          const handler = handlers[requestName] as (...args: any[]) => any
          const responseData = routeDef.payload ? await handler(payload) : await handler()

          await onResponseSent?.({
            ...requestInput,
            response: responseData,
            durationMs: Date.now() - startedAt,
          })

          sendJson(res, 200, responseData)
        } catch (error) {
          await onRequestFailed?.({
            ...requestInput,
            error,
            durationMs: Date.now() - startedAt,
          })
          throw error
        }
      }

      if (runHandler) {
        await runHandler(requestInput, processRequest)
      } else {
        await processRequest()
      }
    } catch (error) {
      const { statusCode, message } = getErrorResponse(error)
      sendJson(res, statusCode, { error: message })
    }
  }

  async function handleStream(
    req: http.IncomingMessage,
    res: http.ServerResponse,
    url: URL,
  ): Promise<void> {
    const name = url.searchParams.get("name")
    if (!name || !Object.hasOwn(schema.streams, name)) {
      res.writeHead(400, { "Content-Type": "text/plain" })
      res.end("Invalid stream name")
      return
    }

    let filter: InferStreamFilter<TSchema, ValidStreamName<TSchema>> | undefined
    try {
      const { filter: filterSchema } = getStreamSchemas(schema, name as ValidStreamName<TSchema>)
      const rawFilter = url.searchParams.get("filter")
      if (rawFilter && !filterSchema) {
        throw new IpcClientError(`Stream ${name} does not accept filter params`)
      }

      let parsedFilter: unknown
      if (rawFilter) {
        try {
          parsedFilter = JSON.parse(rawFilter)
        } catch (error) {
          throw toClientError(error, "Stream filter must be valid JSON")
        }
      }

      filter =
        rawFilter && filterSchema
          ? (filterSchema.parse(parsedFilter) as InferStreamFilter<
              TSchema,
              ValidStreamName<TSchema>
            >)
          : undefined
    } catch (error) {
      const { statusCode, message } = getErrorResponse(
        error instanceof IpcClientError ? error : toClientError(error, "Stream filter is invalid"),
      )
      res.writeHead(statusCode, { "Content-Type": "text/plain" })
      res.end(message)
      return
    }

    try {
      await onSubscribe?.({
        name: name as ValidStreamName<TSchema>,
        filter,
      })
    } catch (error) {
      const { statusCode, message } = getErrorResponse(error)
      res.writeHead(statusCode, { "Content-Type": "text/plain" })
      res.end(message)
      return
    }

    res.writeHead(200, {
      "Content-Type": "application/x-ndjson",
      "Cache-Control": "no-cache",
      Connection: "keep-alive",
    })
    res.flushHeaders()

    const client = { name, filter, res }
    streamClients.add(client)

    const removeClient = () => {
      streamClients.delete(client)
    }

    req.on("close", removeClient)
    res.on("close", removeClient)
  }

  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost")

    if (req.method === "POST" && url.pathname === "/") {
      void handleRequest(req, res)
      return
    }

    if (req.method === "GET" && url.pathname === "/stream") {
      void handleStream(req, res, url)
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
