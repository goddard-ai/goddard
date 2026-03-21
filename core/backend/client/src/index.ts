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

/** Browser-compatible WebSocket constructor used for live stream subscriptions. */
type WebSocketConstructor = typeof WebSocket

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

const STREAM_HEARTBEAT_INTERVAL_MS = 30_000
const STREAM_RECONNECT_DELAYS_MS = [250, 500, 1_000, 2_000, 5_000] as const

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
        const WebSocketImpl = requireWebSocketConstructor()
        const streamUrl = createStreamWebSocketUrl(options.baseUrl, token)
        let socket: WebSocket | null = null
        let heartbeatTimer: ReturnType<typeof setInterval> | undefined
        let reconnectTimer: ReturnType<typeof setTimeout> | undefined
        let reconnectAttempts = 0
        let manualClose = false

        const stopHeartbeat = () => {
          if (!heartbeatTimer) {
            return
          }

          clearInterval(heartbeatTimer)
          heartbeatTimer = undefined
        }

        const scheduleReconnect = (subscription: BackendStreamSubscription) => {
          if (manualClose || reconnectTimer || subscription.isClosed()) {
            return
          }

          const delay =
            STREAM_RECONNECT_DELAYS_MS[
              Math.min(reconnectAttempts, STREAM_RECONNECT_DELAYS_MS.length - 1)
            ]
          reconnectAttempts += 1
          reconnectTimer = setTimeout(() => {
            reconnectTimer = undefined
            void connect(subscription).catch((error) => {
              subscription.emit("error", error)
              scheduleReconnect(subscription)
            })
          }, delay)
        }

        const connect = async (subscription: BackendStreamSubscription): Promise<void> => {
          await new Promise<void>((resolve, reject) => {
            let opened = false
            let lastError: Error | undefined
            const nextSocket = new WebSocketImpl(streamUrl)

            nextSocket.onopen = () => {
              opened = true
              reconnectAttempts = 0
              socket = nextSocket
              stopHeartbeat()
              heartbeatTimer = setInterval(() => {
                if (socket?.readyState === 1) {
                  socket.send("ping")
                }
              }, STREAM_HEARTBEAT_INTERVAL_MS)
              subscription.emit("open")
              resolve()
            }

            nextSocket.onmessage = (event) => {
              const payload = toWebSocketMessageText(event.data)
              if (payload === "pong") {
                return
              }

              try {
                emitStreamMessage(subscription, JSON.parse(payload) as StreamMessage)
              } catch (error) {
                subscription.emit("error", new Error(`Invalid stream payload: ${String(error)}`))
              }
            }

            nextSocket.onerror = () => {
              lastError = new Error("Stream socket error")
              if (opened) {
                subscription.emit("error", lastError)
              }
            }

            nextSocket.onclose = (event) => {
              if (socket === nextSocket) {
                socket = null
              }
              stopHeartbeat()

              if (manualClose || subscription.isClosed()) {
                return
              }

              const closeError =
                lastError ??
                new Error(
                  `Stream socket closed (${event.code}${event.reason ? `: ${event.reason}` : ""})`,
                )

              if (!opened) {
                reject(closeError)
                return
              }

              scheduleReconnect(subscription)
            }
          })
        }

        const subscription = new BackendStreamSubscription(() => {
          manualClose = true
          stopHeartbeat()
          if (reconnectTimer) {
            clearTimeout(reconnectTimer)
            reconnectTimer = undefined
          }
          if (socket && socket.readyState < 2) {
            socket.close()
          }
          socket = null
        })

        await connect(subscription)
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

/** Resolves the global WebSocket implementation required for live stream subscriptions. */
function requireWebSocketConstructor(): WebSocketConstructor {
  if (typeof WebSocket === "undefined") {
    throw new Error("WebSocket is not available in this runtime")
  }

  return WebSocket
}

/** Converts the configured backend base URL into the matching WebSocket stream URL. */
function createStreamWebSocketUrl(baseUrl: string, token: string): string {
  const url = new URL("/stream", baseUrl)
  url.protocol =
    url.protocol === "https:" ? "wss:" : url.protocol === "http:" ? "ws:" : url.protocol
  url.searchParams.set("token", token)
  return url.toString()
}

/** Emits one parsed stream message through the stable subscription event surface. */
function emitStreamMessage(
  subscription: BackendStreamSubscription,
  streamMessage: StreamMessage,
): void {
  subscription.emit("message", streamMessage)
  subscription.emit("event", streamMessage.event)
  subscription.emit(streamMessage.event.type, streamMessage.event)
}

/** Converts an inbound WebSocket frame payload into the UTF-8 text expected by the stream contract. */
function toWebSocketMessageText(data: unknown): string {
  if (typeof data === "string") {
    return data
  }

  if (data instanceof ArrayBuffer) {
    return new TextDecoder().decode(new Uint8Array(data))
  }

  if (ArrayBuffer.isView(data)) {
    return new TextDecoder().decode(data)
  }

  throw new Error(`Unsupported WebSocket payload type: ${typeof data}`)
}
