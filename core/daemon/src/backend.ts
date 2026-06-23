import { authBackendRoutes } from "@goddard-ai/auth/backend"
import {
  composeBackendRoutes,
  createBackendClient as createRouteClient,
  type RouzerClient,
} from "@goddard-ai/backend-plugin"
import { pullRequestBackendRoutes } from "@goddard-ai/pull-request/backend"
import {
  remoteRepoBackendRoutes,
  type RemoteRepoBackendEvent,
} from "@goddard-ai/remote-repo/backend"
import { getErrorMessage } from "radashi"

import { createDebug } from "./logging.ts"

/** Fetch implementation consumed by the daemon's backend client. */
type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>

/** Listener signature used by daemon-owned backend stream subscriptions. */
type StreamHandler = (event?: unknown) => void

const notAuthenticatedMessage = "Not authenticated. Run login first."

type BackendStreamEvent = RemoteRepoBackendEvent

/** Backend routes available to daemon-owned backend clients. */
export const backendRoutes = composeBackendRoutes([
  authBackendRoutes,
  pullRequestBackendRoutes,
  remoteRepoBackendRoutes,
])

/** Error thrown when a daemon backend call requires a login session that is not available. */
export class BackendUnauthenticatedError extends Error {
  constructor(message = notAuthenticatedMessage) {
    super(message)
    this.name = "BackendUnauthenticatedError"
  }
}

/** Returns whether a daemon backend failure was caused by missing or invalid authentication. */
export function isBackendUnauthenticatedError(error: unknown) {
  return (
    error instanceof BackendUnauthenticatedError ||
    (error instanceof Error && error.name === "BackendUnauthenticatedError")
  )
}

/** Constructor options for the daemon's direct backend client. */
export type BackendClientOptions = {
  baseUrl: string
  fetchImpl?: FetchLike
  getAuthorizationHeader?: () => Promise<string | null> | string | null
}

/** Disposable SSE subscription returned by the daemon's backend client. */
export type StreamSubscription = {
  on: (eventName: string, handler: StreamHandler) => StreamSubscription
  off: (eventName: string, handler: StreamHandler) => StreamSubscription
  emit: (eventName: string, payload?: unknown) => void
  close: () => void
  isClosed: () => boolean
}

/** Direct backend client surface owned privately by the daemon. */
export type BackendClient = RouzerClient<typeof backendRoutes> & {
  stream: {
    subscribe: () => Promise<StreamSubscription>
  }
}

/** In-memory SSE subscription wrapper used for daemon-owned repo stream listeners. */
class BackendStreamSubscription implements StreamSubscription {
  #dispose: () => void
  #listeners = new Map<string, Set<StreamHandler>>()
  #isClosed = false

  constructor(dispose: () => void) {
    this.#dispose = dispose
  }

  on(eventName: string, handler: StreamHandler): this {
    const listeners = this.#listeners.get(eventName) ?? new Set<StreamHandler>()
    listeners.add(handler)
    this.#listeners.set(eventName, listeners)
    return this
  }

  off(eventName: string, handler: StreamHandler): this {
    this.#listeners.get(eventName)?.delete(handler)
    return this
  }

  emit(eventName: string, payload?: unknown): void {
    this.#listeners.get(eventName)?.forEach((listener) => listener(payload))
  }

  close(): void {
    if (this.#isClosed) {
      return
    }

    this.#isClosed = true
    this.#dispose()
    this.emit("close")
  }

  isClosed(): boolean {
    return this.#isClosed
  }
}

/** Creates the daemon's direct rouzer-backed client for backend auth, PR, and stream routes. */
export function createBackendClient(options: BackendClientOptions): BackendClient {
  const debug = createDebug("backend.stream")
  const routeClient = createRouteClient({
    baseURL: options.baseUrl,
    fetch: createAuthorizedFetch(options) as typeof fetch,
    routes: backendRoutes,
  })
  const authorizedClient = createAuthorizedRouteClient(backendRoutes, routeClient, options)

  return Object.assign(authorizedClient, {
    stream: {
      subscribe: async () => {
        const abortController = new AbortController()
        debug("backend.stream.subscribe_started")
        const response = await authorizedClient.remoteRepo.stream(undefined, {
          signal: abortController.signal,
        })

        if (!response.ok) {
          const message = `Stream request failed (${response.status}): ${await response.text()}`
          if (response.status === 401) {
            throw new BackendUnauthenticatedError(message)
          }
          throw new Error(message)
        }

        if (!response.body) {
          throw new Error("Stream response did not include a body")
        }

        debug("backend.stream.opened", {
          status: response.status,
        })
        const body = response.body
        const reader = response.body.getReader()
        const subscription = new BackendStreamSubscription(() => {
          debug("backend.stream.close_requested")
          abortController.abort()
          // Different runtimes observe SSE teardown at different layers; cancel all of them so
          // long-lived stream responses do not keep tests or local daemon shutdowns alive.
          void body.cancel().catch(() => {})
          void reader.cancel().catch(() => {})
        })

        subscription.emit("open")
        void consumeSseResponse(reader, subscription, abortController.signal, debug)

        return subscription
      },
    },
  })
}

function createAuthorizedRouteClient(
  routes: Record<string, any>,
  source: Record<string, any>,
  options: BackendClientOptions,
) {
  const context: Record<string, unknown> = {}

  for (const [key, route] of Object.entries(routes)) {
    if (!route || typeof route !== "object") {
      continue
    }
    if (route.kind === "resource") {
      context[key] = createAuthorizedRouteClient(route.children, source[key], options)
      continue
    }
    context[key] = async (input?: Record<string, any>, requestOptions?: RequestInit) => {
      return source[key](input, await addAuthorizationHeader(requestOptions, options, route))
    }
  }

  return context as RouzerClient<typeof backendRoutes>
}

async function addAuthorizationHeader(
  requestOptions: RequestInit | undefined,
  options: BackendClientOptions,
  route: Record<string, any>,
) {
  const authorization = await options.getAuthorizationHeader?.()
  if (!authorization) {
    if (routeRequiresAuthorizationHeader(route)) {
      throw new BackendUnauthenticatedError()
    }

    return requestOptions
  }

  return {
    ...requestOptions,
    headers: {
      ...requestOptions?.headers,
      authorization,
    },
  }
}

function routeRequiresAuthorizationHeader(route: Record<string, any>) {
  return Boolean(route.schema?.headers?.shape?.authorization)
}

function createAuthorizedFetch(options: BackendClientOptions): FetchLike {
  const fetchImpl = options.fetchImpl ?? fetch

  return async (input, init) => {
    const response = await fetchImpl(input, {
      ...init,
    })
    if (response.status !== 401) {
      return response
    }

    throw new BackendUnauthenticatedError(
      `Backend request failed (${response.status}): ${await response.text()}`,
    )
  }
}

/** Reads the backend SSE response stream until the subscription closes or the stream ends. */
async function consumeSseResponse(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  subscription: BackendStreamSubscription,
  signal: AbortSignal,
  debug: (event: string, fields?: Record<string, unknown>) => void,
): Promise<void> {
  const decoder = new TextDecoder()
  let buffer = ""

  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) {
        break
      }

      debug("backend.stream.chunk_read", {
        byteLength: value.byteLength,
      })
      buffer += decoder.decode(value, { stream: true })
      buffer = flushSseBuffer(buffer, subscription, debug)
    }

    buffer += decoder.decode()
    flushSseBuffer(buffer, subscription, debug)
  } catch (error) {
    if (!signal.aborted) {
      debug("backend.stream.read_failed", {
        errorMessage: getErrorMessage(error),
      })
      subscription.emit("error", error)
    }
  } finally {
    const aborted = signal.aborted
    await reader.cancel().catch(() => {})
    if (!subscription.isClosed()) {
      subscription.close()
    }
    debug("backend.stream.closed", {
      aborted,
    })
  }
}

/** Emits complete SSE messages from buffered stream content and preserves trailing partial frames. */
function flushSseBuffer(
  buffer: string,
  subscription: BackendStreamSubscription,
  debug: (event: string, fields?: Record<string, unknown>) => void,
): string {
  let remaining = buffer

  while (true) {
    const match = remaining.match(/\r?\n\r?\n/)
    if (!match || match.index === undefined) {
      return remaining
    }

    const chunk = remaining.slice(0, match.index)
    remaining = remaining.slice(match.index + match[0].length)

    const data = parseSseData(chunk)
    if (!data) {
      continue
    }

    try {
      const parsed = JSON.parse(data) as BackendStreamEvent
      debug("backend.stream.event_received", {
        eventName: parsed.name,
      })
      subscription.emit("event", parsed)
      subscription.emit(parsed.name, parsed)
    } catch (error) {
      debug("backend.stream.event_parse_failed", {
        errorMessage: getErrorMessage(error),
      })
      subscription.emit("error", new Error(`Invalid stream payload: ${getErrorMessage(error)}`))
    }
  }
}

/** Extracts the SSE data payload lines from one event frame. */
function parseSseData(chunk: string): string | null {
  const lines = chunk.split(/\r?\n/)
  const dataLines: string[] = []

  for (const line of lines) {
    if (!line || line.startsWith(":")) {
      continue
    }

    if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trimStart())
    }
  }

  if (dataLines.length === 0) {
    return null
  }

  return dataLines.join("\n")
}
