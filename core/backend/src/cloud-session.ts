import {
  CloudSessionCommand,
  CloudSessionHarnessMessage,
  CloudSessionSyncResponse,
  CreateCloudSessionRequest,
  type CloudSessionCommand as CloudSessionCommandInput,
  type CloudSessionCommandResponse,
  type CloudSessionEvent,
  type CloudSessionHarnessMessage as CloudSessionHarnessMessageInput,
  type CloudSessionSnapshot,
  type CreateCloudSessionRequest as CreateCloudSessionRequestInput,
  type CreateCloudSessionResponse,
} from "@goddard-ai/cloud-session/schema"

/** Socket-like channel used to deliver coordinator commands to the sandbox harness. */
export interface CloudSessionHarnessSocket {
  send(payload: string): void
  close?(code?: number, reason?: string): void
}

/** Durable storage seam for one cloud-session coordinator instance. */
export interface CloudSessionStore {
  load(): Promise<StoredCloudSessionState> | StoredCloudSessionState
  save(state: StoredCloudSessionState): Promise<void> | void
}

/** Stored command metadata used for idempotent daemon retries. */
export type StoredCloudSessionCommand = {
  command: CloudSessionCommandInput
  eventSeq: number
  createdAt: string
  deliveredAt?: string
}

/** Complete durable state for one cloud-session coordinator. */
export type StoredCloudSessionState = {
  session: CloudSessionSnapshot | null
  events: CloudSessionEvent[]
  commands: Record<string, StoredCloudSessionCommand>
}

/** In-memory store used by tests and local direct construction of the Durable Object. */
export class InMemoryCloudSessionStore implements CloudSessionStore {
  #state = createEmptyState()

  load() {
    return cloneState(this.#state)
  }

  save(state: StoredCloudSessionState) {
    this.#state = cloneState(state)
  }
}

/** Coordinates one cloud-owned agent session through an event log and harness channel. */
export class CloudSessionCoordinator {
  readonly #store: CloudSessionStore
  #harness:
    | {
        socket: CloudSessionHarnessSocket
        epoch: number
      }
    | undefined

  constructor(store: CloudSessionStore = new InMemoryCloudSessionStore()) {
    this.#store = store
  }

  async createSession(input: CreateCloudSessionRequestInput = {}) {
    const body = CreateCloudSessionRequest.parse(input)
    return await this.#mutate((state) => {
      if (state.session) {
        return {
          session: state.session,
          events: state.events,
        } satisfies CreateCloudSessionResponse
      }

      const now = new Date().toISOString()
      const session: CloudSessionSnapshot = {
        id: body.sessionId ?? createCloudSessionId(),
        status: "creating",
        sandboxStatus: "pending",
        harnessEpoch: 0,
        lastSeq: 0,
        createdAt: now,
        updatedAt: now,
        metadata: body.metadata,
      }

      state.session = session
      const event = this.#appendEvent(state, {
        at: now,
        source: "coordinator",
        type: "cloud-session.created",
        payload: { session },
      })

      return {
        session: state.session,
        events: [event],
      } satisfies CreateCloudSessionResponse
    })
  }

  async sync(after: number, limit = 100) {
    const state = await this.#store.load()
    const session = assertSession(state)
    const normalizedLimit = Math.min(Math.max(Math.trunc(limit), 1), 500)
    const events = state.events.filter((event) => event.seq > after).slice(0, normalizedLimit)
    const cursor = events.at(-1)?.seq ?? after
    const hasMore = state.events.some((event) => event.seq > cursor)

    return CloudSessionSyncResponse.parse({
      session,
      events,
      cursor,
      hasMore,
    })
  }

  async enqueueCommand(command: CloudSessionCommandInput) {
    const body = CloudSessionCommand.parse(command)
    const result = await this.#mutate((state) => {
      assertSession(state)

      const existing = state.commands[body.commandId]
      if (existing) {
        return {
          commandId: body.commandId,
          duplicate: true,
          accepted: true,
          event: state.events.find((event) => event.seq === existing.eventSeq),
        } satisfies CloudSessionCommandResponse
      }

      const event = this.#appendEvent(state, {
        source: "local-daemon",
        type: "cloud-session.command.accepted",
        payload: { command: body },
      })
      state.commands[body.commandId] = {
        command: body,
        eventSeq: event.seq,
        createdAt: event.at,
      }

      return {
        commandId: body.commandId,
        duplicate: false,
        accepted: true,
        event,
      } satisfies CloudSessionCommandResponse
    })

    await this.#deliverCommand(body)

    return result
  }

  async attachHarness(socket: CloudSessionHarnessSocket) {
    const result = await this.#mutate((state) => {
      const session = assertSession(state)
      const now = new Date().toISOString()
      const nextEpoch = session.harnessEpoch + 1

      state.session = {
        ...session,
        status: "running",
        sandboxStatus: "ready",
        harnessEpoch: nextEpoch,
        updatedAt: now,
      }
      const event = this.#appendEvent(state, {
        at: now,
        source: "coordinator",
        type: "cloud-session.harness.attached",
        payload: { harnessEpoch: nextEpoch },
      })

      return { session: state.session, event }
    })

    this.#harness?.socket.close?.(1012, "Harness superseded")
    this.#harness = {
      socket,
      epoch: result.session.harnessEpoch,
    }
    socket.send(
      JSON.stringify({
        type: "coordinator.hello",
        harnessEpoch: result.session.harnessEpoch,
        sessionId: result.session.id,
      }),
    )

    return result
  }

  isActiveHarness(epoch: number) {
    return this.#harness?.epoch === epoch
  }

  async detachHarness(epoch: number, reason = "disconnected") {
    if (this.#harness?.epoch === epoch) {
      this.#harness = undefined
    }

    return await this.#mutate((state) => {
      const session = assertSession(state)
      if (session.harnessEpoch !== epoch) {
        return { session, stale: true }
      }

      state.session = {
        ...session,
        status: session.status === "ended" ? session.status : "idle",
        sandboxStatus: session.sandboxStatus === "stopped" ? "stopped" : "disconnected",
        updatedAt: new Date().toISOString(),
      }
      const event = this.#appendEvent(state, {
        source: "coordinator",
        type: "cloud-session.harness.detached",
        payload: { harnessEpoch: epoch, reason },
      })

      return { session: state.session, event, stale: false }
    })
  }

  async ingestHarnessMessage(message: CloudSessionHarnessMessageInput) {
    const body = CloudSessionHarnessMessage.parse(message)

    return await this.#mutate((state) => {
      const session = assertSession(state)
      const now = new Date().toISOString()

      if (body.type === "status") {
        state.session = {
          ...session,
          status: body.status,
          sandboxStatus: body.sandboxStatus ?? session.sandboxStatus,
          updatedAt: now,
        }

        return this.#appendEvent(state, {
          at: now,
          source: "harness",
          type: "cloud-session.status",
          payload: {
            status: body.status,
            sandboxStatus: body.sandboxStatus,
            detail: body.detail,
          },
        })
      }

      if (body.type === "error") {
        state.session = {
          ...session,
          status: "failed",
          sandboxStatus: "failed",
          updatedAt: now,
        }

        return this.#appendEvent(state, {
          at: now,
          source: "harness",
          type: "cloud-session.error",
          payload: { message: body.message, payload: body.payload },
        })
      }

      return this.#appendEvent(state, {
        at: now,
        source: "harness",
        type: body.eventType,
        payload: body.payload,
      })
    })
  }

  async #deliverCommand(command: CloudSessionCommandInput) {
    if (!this.#harness) {
      return
    }

    try {
      this.#harness.socket.send(
        JSON.stringify({
          type: "command",
          harnessEpoch: this.#harness.epoch,
          command,
        }),
      )
      await this.#markDelivered(command.commandId)
    } catch {
      this.#harness.socket.close?.(1011, "Failed to deliver command")
      this.#harness = undefined
    }
  }

  async #markDelivered(commandId: string) {
    await this.#mutate((state) => {
      const record = state.commands[commandId]
      if (!record) {
        return
      }

      state.commands[commandId] = {
        ...record,
        deliveredAt: new Date().toISOString(),
      }
    })
  }

  #appendEvent(
    state: StoredCloudSessionState,
    event: Omit<CloudSessionEvent, "seq" | "at"> & { at?: string },
  ) {
    const session = assertSession(state)
    const seq = session.lastSeq + 1
    const at = event.at ?? new Date().toISOString()
    const nextEvent: CloudSessionEvent = {
      seq,
      at,
      source: event.source,
      type: event.type,
      payload: event.payload,
    }

    state.events.push(nextEvent)
    state.session = {
      ...session,
      lastSeq: seq,
      updatedAt: at,
    }

    return nextEvent
  }

  async #mutate<T>(callback: (state: StoredCloudSessionState) => T) {
    const state = await this.#store.load()
    const result = callback(state)
    await this.#store.save(state)
    return result
  }
}

/** Durable Object wrapper that owns one cloud session's coordinator state. */
export class CloudSession {
  readonly #coordinator: CloudSessionCoordinator

  constructor(state?: DurableObjectState) {
    this.#coordinator = new CloudSessionCoordinator(
      state ? new DurableObjectCloudSessionStore(state.storage) : undefined,
    )
  }

  fetch(request: Request) {
    const url = new URL(request.url)
    if (request.method === "POST" && url.pathname === "/create") {
      return this.#create(request)
    }
    if (request.method === "GET" && url.pathname === "/sync") {
      return this.#sync(url)
    }
    if (request.method === "POST" && url.pathname === "/commands") {
      return this.#command(request)
    }
    if (request.method === "GET" && url.pathname === "/harness") {
      return this.#harness(request)
    }

    return new Response("Not found", { status: 404 })
  }

  async #create(request: Request) {
    try {
      const input = await readJson(request, CreateCloudSessionRequest)
      return Response.json(await this.#coordinator.createSession(input))
    } catch (error) {
      return toErrorResponse(error)
    }
  }

  async #sync(url: URL) {
    try {
      const after = Number(url.searchParams.get("after") ?? "0")
      const limit = Number(url.searchParams.get("limit") ?? "100")

      if (!Number.isInteger(after) || after < 0) {
        return Response.json({ error: "after must be a non-negative integer" }, { status: 400 })
      }
      if (!Number.isInteger(limit) || limit < 1) {
        return Response.json({ error: "limit must be a positive integer" }, { status: 400 })
      }

      return Response.json(await this.#coordinator.sync(after, limit))
    } catch (error) {
      return toErrorResponse(error)
    }
  }

  async #command(request: Request) {
    try {
      const command = await readJson(request, CloudSessionCommand)
      return Response.json(await this.#coordinator.enqueueCommand(command))
    } catch (error) {
      return toErrorResponse(error)
    }
  }

  async #harness(request: Request) {
    try {
      const pair = createWebSocketPair()
      const client = pair[0]
      const server = pair[1]
      const acceptedServer = server as WebSocket & { accept(): void }

      acceptedServer.accept()
      const { session } = await this.#coordinator.attachHarness(server)
      const epoch = session.harnessEpoch

      server.addEventListener("message", (event) => {
        void this.#handleHarnessMessage(epoch, event.data)
      })
      server.addEventListener("close", () => {
        void this.#coordinator.detachHarness(epoch)
      })
      request.signal.addEventListener(
        "abort",
        () => {
          void this.#coordinator.detachHarness(epoch, "request-aborted")
        },
        { once: true },
      )

      return new Response(null, { status: 101, webSocket: client })
    } catch (error) {
      return toErrorResponse(error)
    }
  }

  async #handleHarnessMessage(epoch: number, raw: unknown) {
    try {
      if (!this.#coordinator.isActiveHarness(epoch)) {
        return
      }

      const message = JSON.parse(String(raw))
      await this.#coordinator.ingestHarnessMessage(message)
    } catch (error) {
      await this.#coordinator.ingestHarnessMessage({
        type: "error",
        message: `Invalid harness message: ${(error as Error).message}`,
      })
      await this.#coordinator.detachHarness(epoch, "invalid-message")
    }
  }
}

class DurableObjectCloudSessionStore implements CloudSessionStore {
  readonly #storage: DurableObjectStorage

  constructor(storage: DurableObjectStorage) {
    this.#storage = storage
  }

  async load() {
    const state = (await this.#storage.get("cloud-session-state")) as
      | StoredCloudSessionState
      | undefined

    return cloneState(state)
  }

  async save(state: StoredCloudSessionState) {
    await this.#storage.put("cloud-session-state", cloneState(state))
  }
}

/** Creates a coordinator-owned id that is safe for route segments and Durable Object names. */
export function createCloudSessionId() {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return `cls_${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`
}

function createEmptyState() {
  const state: StoredCloudSessionState = {
    session: null,
    events: [],
    commands: {},
  }

  return state
}

function assertSession(state: StoredCloudSessionState) {
  if (!state.session) {
    throw new Error("Cloud session has not been created")
  }

  return state.session
}

function cloneState(state: StoredCloudSessionState | undefined) {
  return state ? structuredClone(state) : createEmptyState()
}

async function readJson<T>(request: Request, schema: { parse(input: unknown): T }) {
  return schema.parse(await request.json())
}

function createWebSocketPair() {
  if (typeof WebSocketPair === "undefined") {
    throw new Error("WebSocketPair is not available in this runtime")
  }

  return new WebSocketPair()
}

function toErrorResponse(error: unknown) {
  const message = error instanceof Error ? error.message : "Unknown error"
  const status = message === "Cloud session has not been created" ? 404 : 400
  return Response.json({ error: message }, { status })
}
