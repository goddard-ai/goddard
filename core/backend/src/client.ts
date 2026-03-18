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
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage"
import { createClient } from "rouzer"

// Fetch implementation used by the backend client.
type FetchLike = typeof fetch

// Listener shape for stream event subscriptions.
type StreamHandler = (event?: unknown) => void

// Constructor options for the backend client.
type BackendClientOptions = {
  baseUrl: string
  tokenStorage?: TokenStorage
  fetchImpl?: FetchLike
}

// Disposable SSE subscription returned by the backend client.
export type StreamSubscription = {
  on: (eventName: string, handler: StreamHandler) => StreamSubscription
  off: (eventName: string, handler: StreamHandler) => StreamSubscription
  emit: (eventName: string, payload?: unknown) => void
  close: () => void
  isClosed: () => boolean
}

// Public backend client surface shared by the daemon and SDK.
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
    subscribeToRepo: (repo: RepoRef) => Promise<StreamSubscription>
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

export function createBackendClient(options: BackendClientOptions): BackendClient {
  const tokenStorage = options.tokenStorage ?? new InMemoryTokenStorage()
  const rouzerClient = createClient({
    baseURL: options.baseUrl,
    fetch: options.fetchImpl ?? fetch,
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
      subscribeToRepo: async ({ owner, repo }) => {
        const token = await requireToken(tokenStorage)
        const abortController = new AbortController()
        const response = await rouzerClient.repoStreamRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: { owner, repo },
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
        void consumeSseResponse(reader, subscription, abortController.signal)

        return subscription
      },
    },
  }
}

async function requireToken(tokenStorage: TokenStorage): Promise<string> {
  const token = await tokenStorage.getToken()
  if (!token) {
    throw new Error("Not authenticated. Run login first.")
  }

  return token
}

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

function parseSseData(chunk: string): string | null {
  const lines = chunk.split(/\r?\n/)
  const dataLines: string[] = []

  for (const line of lines) {
    if (!line || line.startsWith(":")) {
      continue
    }

    if (line.startsWith("data:")) {
      dataLines.push(line.slice("data:".length).trimStart())
    }
  }

  return dataLines.length > 0 ? dataLines.join("\n") : null
}
