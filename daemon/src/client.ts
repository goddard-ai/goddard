import * as routes from "@goddard-ai/schema/daemon/routes"
import * as http from "node:http"
import { createClient } from "rouzer"
import { readSocketPathFromDaemonUrl } from "./ipc.ts"

type FetchLike = typeof fetch

export type DaemonRouteClient = ReturnType<typeof createClient<typeof routes>>

export function createDaemonRouteClient(options: {
  daemonUrl: string
  fetchImpl?: FetchLike
}): DaemonRouteClient {
  return createClient({
    baseURL: "http://unix",
    fetch: options.fetchImpl ?? createDaemonSocketFetch(options.daemonUrl),
    routes,
  })
}

export function createDaemonRouteClientFromEnv(
  env: Record<string, string | undefined> = process.env,
): {
  daemonUrl: string
  sessionToken: string
  client: DaemonRouteClient
} {
  const daemonUrl = requiredEnv(env.GODDARD_DAEMON_URL, "GODDARD_DAEMON_URL")
  const sessionToken = requiredEnv(env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")

  return {
    daemonUrl,
    sessionToken,
    client: createDaemonRouteClient({ daemonUrl }),
  }
}

function createDaemonSocketFetch(daemonUrl: string): FetchLike {
  const socketPath = readSocketPathFromDaemonUrl(daemonUrl)

  return async (input, init) => {
    const request = new Request(input, init)
    const url = new URL(request.url)
    const method = request.method.toUpperCase()
    const headers = headersToNodeHeaders(request.headers)
    const body = await readBody(request, method)

    if (body) {
      headers["content-length"] ??= String(body.length)
    }

    return requestViaSocket({
      socketPath,
      path: `${url.pathname}${url.search}`,
      method,
      headers,
      body,
      signal: request.signal,
    })
  }
}

async function readBody(request: Request, method: string): Promise<Buffer | undefined> {
  if (method === "GET" || method === "HEAD") {
    return undefined
  }

  const buffer = Buffer.from(await request.arrayBuffer())
  return buffer.length > 0 ? buffer : undefined
}

function headersToNodeHeaders(headers: Headers): http.OutgoingHttpHeaders {
  const result: http.OutgoingHttpHeaders = {}

  headers.forEach((value, key) => {
    result[key] = value
  })

  return result
}

function requestViaSocket(input: {
  socketPath: string
  path: string
  method: string
  headers: http.OutgoingHttpHeaders
  body?: Buffer
  signal: AbortSignal
}): Promise<Response> {
  return new Promise<Response>((resolve, reject) => {
    const request = http.request(
      {
        socketPath: input.socketPath,
        path: input.path,
        method: input.method,
        headers: input.headers,
      },
      (response) => {
        const chunks: Buffer[] = []
        response.on("data", (chunk) => {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        })
        response.on("end", () => {
          const headers = new Headers()
          for (const [key, value] of Object.entries(response.headers)) {
            if (Array.isArray(value)) {
              for (const part of value) {
                headers.append(key, part)
              }
            } else if (value != null) {
              headers.set(key, String(value))
            }
          }

          resolve(
            new Response(Buffer.concat(chunks), {
              status: response.statusCode ?? 500,
              statusText: response.statusMessage ?? "",
              headers,
            }),
          )
        })
      },
    )

    const onAbort = () => {
      request.destroy(new Error("The daemon request was aborted"))
    }

    if (input.signal.aborted) {
      onAbort()
      return
    }

    input.signal.addEventListener("abort", onAbort, { once: true })

    request.once("error", (error) => {
      input.signal.removeEventListener("abort", onAbort)
      reject(error)
    })

    request.once("close", () => {
      input.signal.removeEventListener("abort", onAbort)
    })

    if (input.body) {
      request.write(input.body)
    }
    request.end()
  })
}

function requiredEnv(value: string | undefined, name: string): string {
  if (!value) {
    throw new Error(`${name} is required`)
  }

  return value
}
