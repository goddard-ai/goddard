/** Connection-scoped daemon terminal registry for IPC request and stream handlers. */
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

import { DaemonTerminalConnection, DaemonTerminalError } from "./runtime.ts"

/** Options that connect terminal connection state to the daemon IPC stream publisher. */
export type DaemonTerminalConnectionRegistryOptions = {
  publishEvent: (event: TerminalDaemonEvent) => void
}

/** Tracks active terminal connections and enforces connection-local instance ownership. */
export class DaemonTerminalConnectionRegistry {
  readonly #terminalConnections = new Map<TerminalConnectionId, DaemonTerminalConnection>()
  readonly #subscribedConnections = new Set<TerminalConnectionId>()
  readonly #publishEvent: (event: TerminalDaemonEvent) => void

  constructor(options: DaemonTerminalConnectionRegistryOptions) {
    this.#publishEvent = options.publishEvent
  }

  get size() {
    return this.#terminalConnections.size
  }

  connect(_request: TerminalConnectRequest = {}): TerminalConnectResponse {
    const connectionId = `term-${randomUUID()}`
    this.#terminalConnections.set(
      connectionId,
      new DaemonTerminalConnection({
        connectionId,
        onEvent: (event) => this.#publishEvent(event),
      }),
    )
    return { connectionId }
  }

  create(request: TerminalCreateRequest): TerminalCreateResponse {
    return {
      terminal: this.#getConnection(request.connectionId).create(request),
    }
  }

  write(request: TerminalInputRequest) {
    this.#getConnection(request.connectionId).write(request)
  }

  resize(request: TerminalResizeRequest) {
    this.#getConnection(request.connectionId).resize(request)
  }

  restart(request: TerminalRestartRequest): TerminalCreateResponse {
    return {
      terminal: this.#getConnection(request.connectionId).restart(request),
    }
  }

  close(request: TerminalCloseRequest) {
    this.#getConnection(request.connectionId).close(request)
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
    this.#getConnection(filter.connectionId)
  }

  streamConnected(filter: TerminalEventStreamFilter | undefined) {
    if (!filter) {
      throw new DaemonTerminalError(
        "unknown-connection",
        "Terminal event stream filter is missing.",
      )
    }
    this.#getConnection(filter.connectionId)
    if (this.#subscribedConnections.has(filter.connectionId)) {
      throw new DaemonTerminalError(
        "duplicate-instance",
        `Terminal connection ${filter.connectionId} already has an active event stream.`,
        filter.connectionId,
      )
    }
    this.#subscribedConnections.add(filter.connectionId)
  }

  streamDisconnected(filter: TerminalEventStreamFilter | undefined) {
    if (!filter) {
      return
    }
    this.#disconnect(filter.connectionId, false)
  }

  closeAll() {
    while (this.#terminalConnections.size > 0) {
      const connectionId = this.#terminalConnections.keys().next().value
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

  #getConnection(connectionId: TerminalConnectionId) {
    const connection = this.#terminalConnections.get(connectionId)
    if (!connection) {
      throw new DaemonTerminalError(
        "unknown-connection",
        `Terminal connection ${connectionId} is not active.`,
        connectionId,
      )
    }
    return connection
  }

  #disconnect(connectionId: TerminalConnectionId, requireActive: boolean) {
    const connection = this.#terminalConnections.get(connectionId)
    if (!connection) {
      if (requireActive) {
        throw new DaemonTerminalError(
          "unknown-connection",
          `Terminal connection ${connectionId} is not active.`,
          connectionId,
        )
      }
      return
    }

    this.#subscribedConnections.delete(connectionId)
    this.#terminalConnections.delete(connectionId)
    connection.closeAll()
  }
}
