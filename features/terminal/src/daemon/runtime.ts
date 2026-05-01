/** Daemon-owned terminal runtime wrappers around `bun-pty`. */
import type {
  TerminalClientFrame,
  TerminalCloseRequest,
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
} from "@goddard-ai/schema/daemon"
import { spawn, type IDisposable, type IExitEvent, type IPty } from "bun-pty"

const DEFAULT_TERMINAL_NAME = "xterm-256color"
const DEFAULT_TERMINAL_DIMENSIONS = {
  cols: 80,
  rows: 24,
}

/** Options used to connect the daemon terminal manager to its eventual websocket owner. */
export type DaemonTerminalManagerOptions = {
  onEvent?: (event: TerminalDaemonEvent) => void
}

/** Error raised when a terminal control frame cannot be applied to daemon runtime state. */
export class DaemonTerminalError extends Error {
  readonly code: TerminalErrorCode
  readonly instanceId: TerminalInstanceId | undefined

  constructor(code: TerminalErrorCode, message: string, instanceId?: TerminalInstanceId) {
    super(message)
    this.name = "DaemonTerminalError"
    this.code = code
    this.instanceId = instanceId
  }
}

/** Daemon-side owner for the PTYs created by one terminal websocket connection. */
export class DaemonTerminalManager {
  readonly #runtimes = new Map<TerminalInstanceId, DaemonTerminalRuntime>()
  readonly #onEvent: (event: TerminalDaemonEvent) => void

  constructor(options: DaemonTerminalManagerOptions = {}) {
    this.#onEvent = options.onEvent ?? (() => {})
  }

  get size() {
    return this.#runtimes.size
  }

  create(request: TerminalCreateRequest) {
    if (this.#runtimes.has(request.instanceId)) {
      throw new DaemonTerminalError(
        "duplicate-instance",
        `Terminal instance ${request.instanceId} already exists on this connection.`,
        request.instanceId,
      )
    }

    const runtime = new DaemonTerminalRuntime(
      request.instanceId,
      request.options ?? {},
      (event) => {
        if (event.type === "terminal.exit") {
          this.#runtimes.delete(event.instanceId)
        }
        this.#onEvent(event)
      },
    )
    this.#runtimes.set(request.instanceId, runtime)
    this.#onEvent({
      type: "terminal.created",
      terminal: runtime.metadata,
    })
    return runtime.metadata
  }

  write(request: TerminalInputRequest) {
    this.#getRuntime(request.instanceId).write(request.data)
  }

  resize(request: TerminalResizeRequest) {
    this.#getRuntime(request.instanceId).resize(request.dimensions)
  }

  restart(request: TerminalRestartRequest) {
    const existing = this.#getRuntime(request.instanceId)
    const options = request.options ?? existing.options
    existing.close()
    this.#runtimes.delete(request.instanceId)
    return this.create({
      type: "terminal.create",
      instanceId: request.instanceId,
      options,
    })
  }

  close(request: TerminalCloseRequest) {
    const runtime = this.#getRuntime(request.instanceId)
    runtime.close()
    this.#runtimes.delete(request.instanceId)
  }

  handle(frame: TerminalClientFrame) {
    switch (frame.type) {
      case "terminal.create":
        return this.create(frame)
      case "terminal.input":
        return this.write(frame)
      case "terminal.resize":
        return this.resize(frame)
      case "terminal.restart":
        return this.restart(frame)
      case "terminal.close":
        return this.close(frame)
    }
  }

  closeAll() {
    const runtimes = [...this.#runtimes.values()]
    this.#runtimes.clear()
    for (const runtime of runtimes) {
      runtime.close()
    }
  }

  #getRuntime(instanceId: TerminalInstanceId) {
    const runtime = this.#runtimes.get(instanceId)
    if (!runtime) {
      throw new DaemonTerminalError(
        "unknown-instance",
        `Terminal instance ${instanceId} does not exist on this connection.`,
        instanceId,
      )
    }
    return runtime
  }
}

/** Runtime wrapper for one live `bun-pty` instance. */
class DaemonTerminalRuntime {
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
    instanceId: TerminalInstanceId,
    options: TerminalSpawnOptions,
    onEvent: (event: TerminalDaemonEvent) => void,
  ) {
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
      instanceId: this.instanceId,
      exitCode: this.#exitCode,
      signal: this.#signal,
    })
    this.#disposeSubscriptions()
  }

  #disposeSubscriptions() {
    while (this.#subscriptions.length > 0) {
      this.#subscriptions.pop()?.dispose()
    }
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
function resolveTerminalEnv(env: Record<string, string> | undefined) {
  const entries = Object.entries(process.env).flatMap(([key, value]) =>
    value === undefined ? [] : [[key, value] as const],
  )
  return {
    ...Object.fromEntries(entries),
    ...env,
  }
}
