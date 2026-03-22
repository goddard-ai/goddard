import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  PullRequestRecord,
  RepoRef,
  StreamMessage,
} from "@goddard-ai/schema/backend"
import * as routes from "@goddard-ai/schema/backend/routes"
import { createClient } from "rouzer"

/** Fetch implementation consumed by the daemon's backend client. */
type FetchLike = typeof fetch

/** Listener signature used by daemon-owned backend stream subscriptions. */
type StreamHandler = (event?: unknown) => void

/** Constructor options for the daemon's direct backend client. */
export type BackendClientOptions = {
  baseUrl: string
  fetchImpl?: FetchLike
  getAuthorizationHeader?: () => Promise<string | null> | string | null
  clearAuthorization?: () => Promise<void> | void
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
export type BackendClient = {
  auth: {
    startDeviceFlow: (input?: DeviceFlowStart) => Promise<DeviceFlowSession>
    completeDeviceFlow: (input: DeviceFlowComplete) => Promise<AuthSession>
    whoami: () => Promise<AuthSession>
    logout: () => Promise<void>
  }
  pr: {
    create: (input: CreatePrInput) => Promise<PullRequestRecord>
    isManaged: (input: RepoRef & { prNumber: number }) => Promise<boolean>
    reply: (input: RepoRef & { prNumber: number; body: string }) => Promise<{ success: boolean }>
  }
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
  const rouzerClient = createClient({
    baseURL: options.baseUrl,
    fetch: options.fetchImpl ?? fetch,
    routes,
  })

  return {
    auth: {
      startDeviceFlow: async (input = {}) =>
        rouzerClient.authDeviceStartRoute.POST({ body: input }),
      completeDeviceFlow: async (input) =>
        rouzerClient.authDeviceCompleteRoute.POST({ body: input }),
      whoami: async () => {
        const authorization = await requireAuthorizationHeader(options.getAuthorizationHeader)
        return rouzerClient.authSessionRoute.GET({
          headers: { authorization },
        })
      },
      logout: async () => {
        await options.clearAuthorization?.()
      },
    },
    pr: {
      create: async (input) => {
        const authorization = await requireAuthorizationHeader(options.getAuthorizationHeader)
        return rouzerClient.prCreateRoute.POST({
          headers: { authorization },
          body: input,
        })
      },
      isManaged: async ({ owner, repo, prNumber }) => {
        const authorization = await requireAuthorizationHeader(options.getAuthorizationHeader)
        const result = await rouzerClient.prManagedRoute.GET({
          headers: { authorization },
          query: { owner, repo, prNumber },
        })
        return result.managed
      },
      reply: async (input) => {
        const authorization = await requireAuthorizationHeader(options.getAuthorizationHeader)
        return rouzerClient.prReplyRoute.POST({
          headers: { authorization },
          body: input,
        })
      },
    },
    stream: {
      subscribe: async () => {
        const authorization = await requireAuthorizationHeader(options.getAuthorizationHeader)
        const abortController = new AbortController()
        const response = await rouzerClient.repoStreamRoute.GET({
          headers: { authorization },
          signal: abortController.signal,
        })

        if (!response.ok) {
          throw new Error(`Stream request failed (${response.status}): ${await response.text()}`)
        }

        if (!response.body) {
          throw new Error("Stream response did not include a body")
        }

        const body = response.body
        const reader = response.body.getReader()
        const subscription = new BackendStreamSubscription(() => {
          abortController.abort()
          void body.cancel().catch(() => {})
          void reader.cancel().catch(() => {})
        })

        subscription.emit("open")
        void consumeSseResponse(reader, subscription, abortController.signal)

        return subscription
      },
    },
  }
}

/** Resolves the injected auth header or fails when the daemon is unauthenticated. */
async function requireAuthorizationHeader(
  getAuthorizationHeader: BackendClientOptions["getAuthorizationHeader"],
): Promise<string> {
  const authorization = await getAuthorizationHeader?.()
  if (!authorization) {
    throw new Error("Not authenticated. Run login first.")
  }

  return authorization
}

/** Reads the backend SSE response stream until the subscription closes or the stream ends. */
async function consumeSseResponse(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  subscription: BackendStreamSubscription,
  signal: AbortSignal,
): Promise<void> {
  const decoder = new TextDecoder()
  let buffer = ""

  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) {
        break
      }

      buffer += decoder.decode(value, { stream: true })
      buffer = flushSseBuffer(buffer, subscription)
    }

    buffer += decoder.decode()
    flushSseBuffer(buffer, subscription)
  } catch (error) {
    if (!signal.aborted) {
      subscription.emit("error", error)
    }
  } finally {
    await reader.cancel().catch(() => {})
    if (!subscription.isClosed()) {
      subscription.close()
    }
  }
}

/** Emits complete SSE messages from buffered stream content and preserves trailing partial frames. */
function flushSseBuffer(buffer: string, subscription: BackendStreamSubscription): string {
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
      const parsed = JSON.parse(data) as StreamMessage
      subscription.emit("event", parsed.event)
      subscription.emit(parsed.event.type, parsed.event)
    } catch (error) {
      subscription.emit("error", new Error(`Invalid stream payload: ${String(error)}`))
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
