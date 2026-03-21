import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  PullRequestRecord,
  RepoRef,
  RepoEventRecord,
  StreamMessage,
} from "@goddard-ai/schema/backend"
import * as routes from "@goddard-ai/schema/backend/routes"
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage"
import { createClient } from "rouzer"

/** Fetch implementation consumed by the backend client. */
type FetchLike = typeof fetch

/** Listener signature used by backend stream subscriptions. */
type StreamHandler = (event?: unknown) => void

/** Constructor options for the backend client. */
type BackendClientOptions = {
  baseUrl: string
  tokenStorage?: TokenStorage
  fetchImpl?: FetchLike
}

/** Disposable live-stream subscription returned by the backend client. */
export type StreamSubscription = {
  on: (eventName: string, handler: StreamHandler) => StreamSubscription
  off: (eventName: string, handler: StreamHandler) => StreamSubscription
  emit: (eventName: string, payload?: unknown) => void
  close: () => void
  isClosed: () => boolean
}

/** Public backend client surface shared by the daemon and SDK. */
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
    history: (input?: { after?: number }) => Promise<RepoEventRecord[]>
    subscribe: () => Promise<StreamSubscription>
  }
}

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

/** Creates a backend client that owns auth, PR, and stream HTTP behavior. */
export function createBackendClient(options: BackendClientOptions): BackendClient {
  const tokenStorage = options.tokenStorage ?? new InMemoryTokenStorage()
  const fetchImpl = options.fetchImpl ?? fetch
  const rouzerClient = createClient({
    baseURL: options.baseUrl,
    fetch: fetchImpl,
    routes,
  })

  return {
    auth: {
      startDeviceFlow: async (input = {}) => {
        return rouzerClient.authDeviceStartRoute.POST({ body: input })
      },
      completeDeviceFlow: async (input) => {
        const session = await rouzerClient.authDeviceCompleteRoute.POST({ body: input })
        await tokenStorage.setToken(session.token)
        return session
      },
      whoami: async () => {
        const token = await requireToken(tokenStorage)
        return rouzerClient.authSessionRoute.GET({
          headers: { authorization: `Bearer ${token}` },
        })
      },
      logout: async () => {
        await tokenStorage.clearToken()
      },
    },
    pr: {
      create: async (input) => {
        const token = await requireToken(tokenStorage)
        return rouzerClient.prCreateRoute.POST({
          headers: { authorization: `Bearer ${token}` },
          body: input,
        })
      },
      isManaged: async ({ owner, repo, prNumber }) => {
        const token = await requireToken(tokenStorage)
        const result = await rouzerClient.prManagedRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: { owner, repo, prNumber },
        })
        return result.managed
      },
      reply: async (input) => {
        const token = await requireToken(tokenStorage)
        return rouzerClient.prReplyRoute.POST({
          headers: { authorization: `Bearer ${token}` },
          body: input,
        })
      },
    },
    stream: {
      history: async (input = {}) => {
        const token = await requireToken(tokenStorage)
        const response = await rouzerClient.repoStreamHistoryRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: input,
        })
        return response.events
      },
      subscribe: async () => {
        const token = await requireToken(tokenStorage)
        const abortController = new AbortController()
        const response = await fetchImpl(new URL("/stream", options.baseUrl).toString(), {
          headers: {
            accept: "application/x-ndjson",
            authorization: `Bearer ${token}`,
          },
          signal: abortController.signal,
        })

        if (!response.ok) {
          throw new Error(`Stream request failed (${response.status}): ${await response.text()}`)
        }

        if (!response.body) {
          throw new Error("Stream response did not include a body")
        }

        const reader = response.body.getReader()
        const subscription = new BackendStreamSubscription(() => {
          abortController.abort()
          void reader.cancel().catch(() => {})
        })

        subscription.emit("open")
        void consumeNdjsonResponse(reader, subscription, abortController.signal)

        return subscription
      },
    },
  }
}

/** Resolves the stored auth token or fails when the client is unauthenticated. */
async function requireToken(tokenStorage: TokenStorage): Promise<string> {
  const token = await tokenStorage.getToken()
  if (!token) {
    throw new Error("Not authenticated. Run login first.")
  }

  return token
}

/** Reads the backend NDJSON response stream until the subscription closes or the stream ends. */
async function consumeNdjsonResponse(
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
      buffer = flushNdjsonBuffer(buffer, subscription)
    }

    buffer += decoder.decode()
    flushNdjsonBuffer(buffer, subscription)
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

/** Emits complete NDJSON messages from the buffered stream content and preserves any trailing partial line. */
function flushNdjsonBuffer(buffer: string, subscription: BackendStreamSubscription): string {
  let remaining = buffer

  while (true) {
    const newlineIndex = remaining.indexOf("\n")
    if (newlineIndex === -1) {
      return remaining
    }

    const chunk = remaining.slice(0, newlineIndex).trim()
    remaining = remaining.slice(newlineIndex + 1)

    if (!chunk) {
      continue
    }

    try {
      const streamMessage = JSON.parse(chunk) as StreamMessage
      subscription.emit("message", streamMessage)
      const parsed = streamMessage
      subscription.emit("event", parsed.event)
      subscription.emit(parsed.event.type, parsed.event)
    } catch (error) {
      subscription.emit("error", new Error(`Invalid stream payload: ${String(error)}`))
    }
  }
}
