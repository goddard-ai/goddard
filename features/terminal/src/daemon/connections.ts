/** Connection-scoped daemon terminal manager registry for IPC request and stream handlers. */
import { randomUUID } from "node:crypto"
import type {
  TerminalCloseRequest,
  TerminalConnectionId,
  TerminalConnectRequest,
  TerminalConnectResponse,
  TerminalCreateRequest,
  TerminalCreateResponse,
  TerminalDaemonEvent,
  TerminalDisconnectRequest,
  TerminalEventStreamFilter,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
} from "@goddard-ai/schema/daemon/terminals"

import { DaemonTerminalError, DaemonTerminalManager } from "./runtime.ts"

/** Options that connect terminal connection state to the daemon IPC stream publisher. */
export type DaemonTerminalConnectionRegistryOptions = {
  publishEvent: (event: TerminalDaemonEvent) => void
}

/** Tracks active terminal connections and enforces connection-local instance ownership. */
export class DaemonTerminalConnectionRegistry {
  readonly #managers = new Map<TerminalConnectionId, DaemonTerminalManager>()
  readonly #streamConnections = new Set<TerminalConnectionId>()
  readonly #publishEvent: (event: TerminalDaemonEvent) => void

  constructor(options: DaemonTerminalConnectionRegistryOptions) {
    this.#publishEvent = options.publishEvent
  }

  get size() {
    return this.#managers.size
  }

  connect(_request: TerminalConnectRequest = {}): TerminalConnectResponse {
    const connectionId = `term-${randomUUID()}`
    this.#managers.set(
      connectionId,
      new DaemonTerminalManager({
        connectionId,
        onEvent: (event) => this.#publishEvent(event),
      }),
    )
    return { connectionId }
  }

  create(request: TerminalCreateRequest): TerminalCreateResponse {
    return {
      terminal: this.#getManager(request.connectionId).create(request),
    }
  }

  write(request: TerminalInputRequest) {
    this.#getManager(request.connectionId).write(request)
  }

  resize(request: TerminalResizeRequest) {
    this.#getManager(request.connectionId).resize(request)
  }

  restart(request: TerminalRestartRequest): TerminalCreateResponse {
    return {
      terminal: this.#getManager(request.connectionId).restart(request),
    }
  }

  close(request: TerminalCloseRequest) {
    this.#getManager(request.connectionId).close(request)
  }

  disconnect(request: TerminalDisconnectRequest) {
    this.#disconnect(request.connectionId, true)
  }

  requireConnection(filter: TerminalEventStreamFilter | undefined) {
    if (!filter) {
      throw new DaemonTerminalError(
        "unknown-connection",
        "Terminal event stream filter is missing.",
      )
    }
    this.#getManager(filter.connectionId)
  }

  streamConnected(filter: TerminalEventStreamFilter | undefined) {
    if (!filter) {
      throw new DaemonTerminalError(
        "unknown-connection",
        "Terminal event stream filter is missing.",
      )
    }
    this.#getManager(filter.connectionId)
    if (this.#streamConnections.has(filter.connectionId)) {
      throw new DaemonTerminalError(
        "duplicate-instance",
        `Terminal connection ${filter.connectionId} already has an active event stream.`,
        filter.connectionId,
      )
    }
    this.#streamConnections.add(filter.connectionId)
  }

  streamDisconnected(filter: TerminalEventStreamFilter | undefined) {
    if (!filter) {
      return
    }
    this.#disconnect(filter.connectionId, false)
  }

  closeAll() {
    while (this.#managers.size > 0) {
      const connectionId = this.#managers.keys().next().value
      if (!connectionId) {
        return
      }
      this.#disconnect(connectionId, false)
    }
  }

  emitRequestError(error: unknown) {
    if (!(error instanceof DaemonTerminalError) || !error.connectionId) {
      return
    }

    this.#publishEvent({
      type: "terminal.error",
      connectionId: error.connectionId,
      instanceId: error.instanceId,
      code: error.code,
      message: error.message,
      recoverable: true,
    })
  }

  #getManager(connectionId: TerminalConnectionId) {
    const manager = this.#managers.get(connectionId)
    if (!manager) {
      throw new DaemonTerminalError(
        "unknown-connection",
        `Terminal connection ${connectionId} is not active.`,
        connectionId,
      )
    }
    return manager
  }

  #disconnect(connectionId: TerminalConnectionId, requireActive: boolean) {
    const manager = this.#managers.get(connectionId)
    if (!manager) {
      if (requireActive) {
        throw new DaemonTerminalError(
          "unknown-connection",
          `Terminal connection ${connectionId} is not active.`,
          connectionId,
        )
      }
      return
    }

    this.#streamConnections.delete(connectionId)
    this.#managers.delete(connectionId)
    manager.closeAll()
  }
}
