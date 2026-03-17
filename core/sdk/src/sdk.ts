import {
  type AuthSession,
  type CreatePrInput,
  type DeviceFlowComplete,
  type DeviceFlowSession,
  type DeviceFlowStart,
  type RepoRef,
  type StreamMessage,
} from "@goddard-ai/schema/backend"
import * as routes from "@goddard-ai/schema/backend/routes"
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage"
import { createClient, type RouteRequest } from "rouzer"

type BackendHttpClient = ReturnType<typeof createClient<typeof routes>>

export type GoddardSdkOptions = {
  backendUrl: string
  tokenStorage?: TokenStorage
  fetch?: typeof globalThis.fetch
}

type StreamHandler = (event?: unknown) => void

export class StreamSubscription {
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

export class GoddardSdk {
  readonly #tokenStorage: TokenStorage
  readonly #backend: BackendHttpClient

  constructor(options: GoddardSdkOptions) {
    this.#tokenStorage = options.tokenStorage ?? new InMemoryTokenStorage()
    this.#backend = createClient({
      baseURL: new URL(options.backendUrl).toString(),
      fetch: options.fetch ?? fetch,
      routes: routes,
    })
  }

  get auth() {
    return {
      startDeviceFlow: async (input: DeviceFlowStart = {}) => {
        return this.#sendJson<DeviceFlowSession>(
          this.#backend.request(routes.authDeviceStartRoute.POST({ body: input })),
        )
      },
      completeDeviceFlow: async (input: DeviceFlowComplete) => {
        const session = await this.#sendJson<AuthSession>(
          this.#backend.request(routes.authDeviceCompleteRoute.POST({ body: input })),
        )
        await this.#tokenStorage.setToken(session.token)
        return session
      },
      login: async ({
        githubUsername,
        onPrompt,
      }: {
        githubUsername: string
        onPrompt: (verificationUri: string, userCode: string) => void
      }) => {
        const start = await this.auth.startDeviceFlow({ githubUsername })
        onPrompt(start.verificationUri, start.userCode)

        const expiresAt = Date.now() + start.expiresIn * 1000
        let delay = start.interval * 1000

        while (Date.now() < expiresAt) {
          try {
            return await this.auth.completeDeviceFlow({
              deviceCode: start.deviceCode,
              githubUsername: githubUsername ?? "",
            })
          } catch (e: any) {
            if (
              e.message &&
              !e.message.includes("authorization_pending") &&
              !e.message.includes("slow_down")
            ) {
              throw e
            }
            if (e.message && e.message.includes("slow_down")) {
              delay += 5000
            }
          }
          await new Promise((resolve) => setTimeout(resolve, delay))
        }
        throw new Error("Device flow authentication timed out.")
      },
      whoami: async () => {
        const token = await this.#requireToken()
        return this.#backend.authSessionRoute.GET({
          headers: { authorization: `Bearer ${token}` },
        })
      },
      logout: async () => {
        await this.#tokenStorage.clearToken()
      },
    }
  }

  get pr() {
    return {
      create: async (input: CreatePrInput) => {
        const token = await this.#requireToken()
        return this.#backend.prCreateRoute.POST({
          headers: { authorization: `Bearer ${token}` },
          body: input,
        })
      },
      isManaged: async ({ owner, repo, prNumber }: RepoRef & { prNumber: number }) => {
        const token = await this.#requireToken()
        const result = await this.#backend.prManagedRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: { owner, repo, prNumber },
        })
        return result.managed
      },
      reply: async (input: { owner: string; repo: string; prNumber: number; body: string }) => {
        const token = await this.#requireToken()
        return this.#backend.prReplyRoute.POST({
          headers: { authorization: `Bearer ${token}` },
          body: input,
        })
      },
    }
  }

  get stream() {
    return {
      subscribeToRepo: async ({ owner, repo }: RepoRef) => {
        const token = await this.#tokenStorage.getToken()
        if (!token) {
          throw new Error("Not authenticated. Run login first.")
        }

        const streamRequest = routes.repoStreamRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: { owner, repo },
        })
        const streamURL = buildRequestURL(this.#backend.config.baseURL, streamRequest)
        const abortController = new AbortController()

        const fetch = this.#backend.config.fetch ?? globalThis.fetch
        const response = await fetch(streamURL, {
          method: "GET",
          headers: {
            accept: "text/event-stream",
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

        const subscription = new StreamSubscription(() => {
          abortController.abort()
        })

        subscription.emit("open")
        void consumeSseResponse(response.body, subscription, abortController.signal)

        return subscription
      },
    }
  }

  async #sendJson<T>(responsePromise: Promise<Response>): Promise<T> {
    const response = await responsePromise

    if (!response.ok) {
      const errorBody = await response.text()
      throw new Error(`Request failed (${response.status}): ${errorBody}`)
    }

    return (await response.json()) as T
  }

  async #requireToken(): Promise<string> {
    const token = await this.#tokenStorage.getToken()
    if (!token) {
      throw new Error("Not authenticated. Run login first.")
    }

    return token
  }
}

function buildRequestURL(baseUrl: string, request: RouteRequest): URL {
  return new URL(request.path.href(request.args.path, request.args.query), baseUrl)
}

async function consumeSseResponse(
  body: ReadableStream<Uint8Array>,
  subscription: StreamSubscription,
  signal: AbortSignal,
): Promise<void> {
  const reader = body.getReader()
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

function flushSseBuffer(buffer: string, subscription: StreamSubscription): string {
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
