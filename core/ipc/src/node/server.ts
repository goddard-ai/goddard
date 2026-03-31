import { existsSync, unlinkSync } from "node:fs"
import * as http from "node:http"
import {
  type AppSchema,
  type InferRequestPayload,
  type InferResponseType,
  type InferStreamPayload,
  type InferStreamSubscription,
  type RequestArguments,
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

/** Creates the Node IPC server for one socket-backed application schema. */
export function createServer<S extends AppSchema>(
  socketPath: string,
  schema: S,
  handlers: Handlers<S>,
) {
  const streamClients = new Set<{
    name: string
    subscription: unknown
    res: http.ServerResponse
  }>()

  function publish<K extends ValidStreamName<S>>(name: K, payload: InferStreamPayload<S, K>) {
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

  const server: http.Server = http.createServer(
    (req: http.IncomingMessage, res: http.ServerResponse) => {
      const url = new URL(req.url ?? "/", "http://localhost")

      if (req.method === "POST" && url.pathname === "/") {
        void (async () => {
          try {
            const body = await readBody(req)
            const message = JSON.parse(body) as { name?: unknown; payload?: unknown }

            if (typeof message.name !== "string") {
              throw new Error("Request name must be a string")
            }

            const routeDef = schema.requests[message.name]
            if (!routeDef) {
              throw new Error(`Unknown request: ${message.name}`)
            }

            const requestName = message.name as ValidRequestName<S>
            const handler = handlers[requestName] as (
              ...args: RequestArguments<S, typeof requestName>
            ) =>
              | Promise<InferResponseType<S, typeof requestName>>
              | InferResponseType<S, typeof requestName>
            const responseData =
              "payload" in routeDef
                ? await handler(
                    routeDef.payload.parse(message.payload) as InferRequestPayload<
                      S,
                      typeof requestName
                    >,
                  )
                : await handler()

            sendJson(res, 200, responseData)
          } catch (error) {
            sendJson(res, 400, { error: getErrorMessage(error) })
          }
        })()
        return
      }

      if (req.method === "GET" && url.pathname === "/stream") {
        const name = url.searchParams.get("name")
        if (!name || !Object.hasOwn(schema.streams, name)) {
          res.writeHead(400, { "Content-Type": "text/plain" })
          res.end("Invalid stream name")
          return
        }

        let subscription: InferStreamSubscription<S, ValidStreamName<S>> | undefined
        try {
          const { subscription: subscriptionSchema } = getStreamSchemas(
            schema,
            name as ValidStreamName<S>,
          )
          const rawSubscription = url.searchParams.get("subscription")
          if (rawSubscription && !subscriptionSchema) {
            throw new Error(`Stream ${name} does not accept subscription params`)
          }
          subscription =
            rawSubscription && subscriptionSchema
              ? (subscriptionSchema.parse(JSON.parse(rawSubscription)) as InferStreamSubscription<
                  S,
                  ValidStreamName<S>
                >)
              : undefined
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
        return
      }

      res.writeHead(404)
      res.end()
    },
  )

  safeUnlink(socketPath)
  server.listen(socketPath)
  server.on("close", () => {
    safeUnlink(socketPath)
  })

  return { server, publish }
}
