/** Daemon-owned terminal wrappers around `bun-pty`. */
import type {
  TerminalCloseRequest,
  TerminalConnectionId,
  TerminalCreateRequest,
  TerminalDaemonEvent,
  TerminalDimensions,
  TerminalErrorCode,
  TerminalInputRequest,
  TerminalInstanceId,
  TerminalResizeRequest,
  TerminalRestartRequest,
  TerminalRuntimeMetadata,
  TerminalSpawnOptions,
} from "@goddard-ai/schema/daemon/terminals"
import { spawn, type IDisposable, type IExitEvent, type IPty } from "bun-pty"

const DEFAULT_TERMINAL_NAME = "xterm-256color"
const DEFAULT_TERMINAL_DIMENSIONS = {
  cols: 80,
  rows: 24,
}

/** Options used to connect one daemon terminal connection to its owning event stream. */
export type DaemonTerminalConnectionOptions = {
  connectionId: TerminalConnectionId
  onEvent?: (event: TerminalDaemonEvent) => void
}

/** Error raised when a terminal control request cannot be applied to daemon terminal state. */
export class DaemonTerminalError extends Error {
  readonly code: TerminalErrorCode
  readonly connectionId: TerminalConnectionId | undefined
  readonly instanceId: TerminalInstanceId | undefined

  constructor(
    code: TerminalErrorCode,
    message: string,
    connectionId?: TerminalConnectionId,
    instanceId?: TerminalInstanceId,
  ) {
    super(message)
    this.name = "DaemonTerminalError"
    this.code = code
    this.connectionId = connectionId
    this.instanceId = instanceId
  }
}

/** Daemon-side owner for the PTYs created by one terminal stream connection. */
export class DaemonTerminalConnection {
  readonly #terminals = new Map<TerminalInstanceId, DaemonTerminal>()
  readonly #connectionId: TerminalConnectionId
  readonly #onEvent: (event: TerminalDaemonEvent) => void

  constructor(options: DaemonTerminalConnectionOptions) {
    this.#connectionId = options.connectionId
    this.#onEvent = options.onEvent ?? (() => {})
  }

  get size() {
    return this.#terminals.size
  }

  create(request: TerminalCreateRequest) {
    this.#assertConnection(request.connectionId)
    if (this.#terminals.has(request.instanceId)) {
      throw new DaemonTerminalError(
        "duplicate-instance",
        `Terminal instance ${request.instanceId} already exists on this connection.`,
        this.#connectionId,
        request.instanceId,
      )
    }

    let terminal: DaemonTerminal
    try {
      terminal = new DaemonTerminal(
        this.#connectionId,
        request.instanceId,
        request.options ?? {},
        (event) => {
          if (event.type === "terminal.exit") {
            this.#terminals.delete(event.instanceId)
          }
          this.#onEvent(event)
        },
      )
    } catch (error) {
      throw new DaemonTerminalError(
        "spawn-failed",
        `Failed to spawn terminal instance ${request.instanceId}: ${
          error instanceof Error ? error.message : String(error)
        }`,
        this.#connectionId,
        request.instanceId,
      )
    }
    this.#terminals.set(request.instanceId, terminal)
    this.#onEvent({
      type: "terminal.created",
      connectionId: this.#connectionId,
      terminal: terminal.metadata,
    })
    return terminal.metadata
  }

  write(request: TerminalInputRequest) {
    this.#assertConnection(request.connectionId)
    this.#getTerminal(request.instanceId).write(request.data)
  }

  resize(request: TerminalResizeRequest) {
    this.#assertConnection(request.connectionId)
    this.#getTerminal(request.instanceId).resize(request.dimensions)
  }

  restart(request: TerminalRestartRequest) {
    this.#assertConnection(request.connectionId)
    const existing = this.#getTerminal(request.instanceId)
    const options = request.options ?? existing.options
    existing.close()
    this.#terminals.delete(request.instanceId)
    return this.create({
      connectionId: this.#connectionId,
      instanceId: request.instanceId,
      options,
    })
  }

  close(request: TerminalCloseRequest) {
    this.#assertConnection(request.connectionId)
    const terminal = this.#getTerminal(request.instanceId)
    terminal.close()
    this.#terminals.delete(request.instanceId)
  }

  closeAll() {
    const terminals = [...this.#terminals.values()]
    this.#terminals.clear()
    for (const terminal of terminals) {
      terminal.close()
    }
  }

  #getTerminal(instanceId: TerminalInstanceId) {
    const terminal = this.#terminals.get(instanceId)
    if (!terminal) {
      throw new DaemonTerminalError(
        "unknown-instance",
        `Terminal instance ${instanceId} does not exist on this connection.`,
        this.#connectionId,
        instanceId,
      )
    }
    return terminal
  }

  #assertConnection(connectionId: TerminalConnectionId) {
    if (connectionId !== this.#connectionId) {
      throw new DaemonTerminalError(
        "unknown-connection",
        `Terminal connection ${connectionId} does not match this terminal connection.`,
        connectionId,
      )
    }
  }
}

/** Wrapper for one live `bun-pty` terminal. */
class DaemonTerminal {
  readonly connectionId: TerminalConnectionId
  readonly instanceId: TerminalInstanceId
  readonly options: TerminalSpawnOptions
  readonly #pty: IPty
  readonly #onEvent: (event: TerminalDaemonEvent) => void
  readonly #subscriptions: IDisposable[]
  #state: TerminalRuntimeMetadata["state"] = "running"
  #exitCode: number | null = null
  #signal: string | null = null
  #closed = false

  constructor(
    connectionId: TerminalConnectionId,
    instanceId: TerminalInstanceId,
    options: TerminalSpawnOptions,
    onEvent: (event: TerminalDaemonEvent) => void,
  ) {
    this.connectionId = connectionId
    this.instanceId = instanceId
    this.options = options
    this.#onEvent = onEvent
    this.#pty = spawn(resolveTerminalCommand(options.command), resolveTerminalArgs(options), {
      name: DEFAULT_TERMINAL_NAME,
      cols: options.dimensions?.cols ?? DEFAULT_TERMINAL_DIMENSIONS.cols,
      rows: options.dimensions?.rows ?? DEFAULT_TERMINAL_DIMENSIONS.rows,
      cwd: options.cwd,
      env: resolveTerminalEnv(options.env),
    })
    this.#subscriptions = [
      this.#pty.onData((data) => {
        if (data.length > 0) {
          this.#onEvent({
            type: "terminal.output",
            connectionId: this.connectionId,
            instanceId: this.instanceId,
            data,
          })
        }
      }),
      this.#pty.onExit((event) => {
        this.#recordExit(event)
      }),
    ]
  }

  get metadata() {
    return {
      instanceId: this.instanceId,
      state: this.#state,
      cwd: this.options.cwd ?? process.cwd(),
      title: this.options.title ?? this.#pty.process,
      dimensions: {
        cols: this.#pty.cols,
        rows: this.#pty.rows,
      },
      exitCode: this.#exitCode,
      signal: this.#signal,
    } satisfies TerminalRuntimeMetadata
  }

  write(data: string) {
    this.#pty.write(data)
  }

  resize(dimensions: TerminalDimensions) {
    this.#pty.resize(dimensions.cols, dimensions.rows)
  }

  close() {
    if (this.#closed) {
      return
    }

    this.#closed = true
    this.#pty.kill()
    this.#disposeSubscriptions()
  }

  #recordExit(event: IExitEvent) {
    this.#state = this.#closed ? "closed" : "exited"
    this.#exitCode = event.exitCode
    this.#signal = event.signal === undefined ? null : String(event.signal)
    this.#onEvent({
      type: "terminal.exit",
      connectionId: this.connectionId,
      instanceId: this.instanceId,
      exitCode: this.#exitCode,
      signal: this.#signal,
    })
    this.#disposeSubscriptions()
  }

  #disposeSubscriptions() {
    for (const subscription of this.#subscriptions) {
      subscription.dispose()
    }
    this.#subscriptions.length = 0
  }
}

/** Resolves the executable used for a terminal when the client did not request one. */
function resolveTerminalCommand(command: string | undefined) {
  return command ?? process.env.SHELL ?? "/bin/sh"
}

/** Resolves shell args without forcing login-shell flags onto explicit commands. */
function resolveTerminalArgs(options: TerminalSpawnOptions) {
  if (options.args) {
    return options.args
  }
  return options.command ? [] : ["-l"]
}

/** Builds a PTY environment that preserves daemon process defaults unless overridden. */
function resolveTerminalEnv(env: Record<string, string> | undefined): Record<string, string> {
  const entries = Object.entries(process.env as Record<string, string | undefined>).flatMap(
    ([key, value]) => (value === undefined ? [] : [[key, value] as const]),
  )
  return {
    ...Object.fromEntries(entries),
    ...env,
  }
}
