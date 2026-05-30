import type {
  TerminalDaemonEvent,
  TerminalRuntimeMetadata as TerminalMetadata,
} from "@goddard-ai/schema/daemon"
import type { GoddardTerminalConnection, GoddardTerminalNamespace } from "@goddard-ai/sdk"
import { listen, Sigma } from "preact-sigma"
import { getErrorMessage } from "radashi"

import { goddardSdk } from "~/sdk.ts"
import { workbenchTabLifecycle } from "~/workbench-tab-lifecycle.ts"
import { TerminalSession, type TerminalViewportChunk } from "./terminal-session.ts"

const DEFAULT_TERMINAL_ID = "main"

export type TerminalTabPayload = {
  tabId: string
  terminalId: string
  title: string
  cwd: string | null
}

type TerminalTabStatus =
  | "idle"
  | "connecting"
  | "starting"
  | "running"
  | "restarting"
  | "exited"
  | "closed"
  | "error"

type TerminalTabState = {
  terminalId: string
  cwd: string | null
  title: string
  status: TerminalTabStatus
  statusMessage: string | null
  cols: number
  rows: number
  exitCode: number | null
  signal: string | null
}

/** One app terminal tab backed by a daemon terminal connection through the shared SDK. */
export class TerminalTab extends Sigma<TerminalTabState> {
  /** Viewport model retained outside mounted components so hidden tabs keep their terminal buffer. */
  readonly terminal = new TerminalSession({
    disposeOnTeardown: false,
    minimumCols: 56,
    minimumRows: 14,
  })
  /** SDK connection owning this tab's daemon terminal stream and terminal instance. */
  #connection: GoddardTerminalConnection | null = null
  /** Stream unsubscribe returned by the SDK terminal subscription. */
  #unsubscribe: (() => void) | null = null
  /** Append-only output chunks replayed into the retained terminal viewport. */
  #chunks: TerminalViewportChunk[] = []
  /** Monotonic chunk id counter used to preserve stable output chunk ordering. */
  #nextChunkId = 0
  /** In-flight start operation reused to avoid duplicate daemon terminals during remounts. */
  #startPromise: Promise<void> | null = null
  /** Whether this tab has been torn down and must ignore late daemon events. */
  #isDisposed = false

  constructor(payload: TerminalTabPayload) {
    super({
      terminalId: payload.terminalId,
      cwd: payload.cwd,
      title: payload.title,
      status: "idle",
      statusMessage: null,
      cols: 56,
      rows: 14,
      exitCode: null,
      signal: null,
    })
  }

  /** Opens the daemon terminal connection and creates the tab's terminal instance once. */
  start() {
    if (this.#isDisposed) {
      return Promise.resolve()
    }

    if (this.status === "running" || this.status === "connecting" || this.#startPromise) {
      return this.#startPromise ?? Promise.resolve()
    }

    this.status = "connecting"
    this.statusMessage = null
    this.exitCode = null
    this.signal = null

    this.#startPromise = this.#connect()
      .catch((error) => {
        if (!this.#isDisposed) {
          this.status = "error"
          this.statusMessage = getErrorMessage(error)
        }
      })
      .finally(() => {
        this.#startPromise = null
      })

    return this.#startPromise
  }

  /** Forwards raw keyboard or paste data to the daemon terminal. */
  async write(data: string) {
    if (!this.#connection || this.#isDisposed || this.status !== "running") {
      return
    }

    await this.#connection.write({
      instanceId: DEFAULT_TERMINAL_ID,
      data,
    })
  }

  /** Forwards viewport fit changes to the daemon terminal. */
  async resize(dimensions: { cols: number; rows: number }) {
    this.cols = dimensions.cols
    this.rows = dimensions.rows

    if (!this.#connection || this.#isDisposed || this.status !== "running") {
      return
    }

    await this.#connection.resize({
      instanceId: DEFAULT_TERMINAL_ID,
      dimensions,
    })
  }

  /** Restarts the current terminal process or starts a new connection after exit or close. */
  async restart() {
    if (this.#isDisposed) {
      return
    }

    this.#clearOutput()

    if (!this.#connection || this.status === "closed" || this.status === "exited") {
      await this.#disconnect()
      await this.start()
      return
    }

    this.status = "restarting"
    this.statusMessage = null
    const response = await this.#connection.restart({
      instanceId: DEFAULT_TERMINAL_ID,
      options: this.#terminalOptions(),
    })
    this.#applyMetadata(response.terminal)
  }

  /** Closes the daemon terminal connection while leaving the tab available for restart. */
  async close() {
    if (this.#isDisposed) {
      return
    }

    await this.#connection?.close({ instanceId: DEFAULT_TERMINAL_ID }).catch(() => {})
    await this.#disconnect()
    this.status = "closed"
    this.statusMessage = null
  }

  /** Tears down terminal resources after the owning workbench tab closes. */
  dispose() {
    if (this.#isDisposed) {
      return
    }

    this.#isDisposed = true
    this.terminal.dispose()
    void this.#disconnect()
  }

  async #connect() {
    const terminal = goddardSdk.terminal as GoddardTerminalNamespace
    const connection = await terminal.connect()

    if (this.#isDisposed) {
      await connection.disconnect().catch(() => {})
      return
    }

    const unsubscribe = await connection.subscribe((event) => {
      this.#handleEvent(event)
    })

    if (this.#isDisposed) {
      unsubscribe()
      await connection.disconnect().catch(() => {})
      return
    }

    this.#connection = connection
    this.#unsubscribe = unsubscribe

    const response = await connection.create({
      instanceId: DEFAULT_TERMINAL_ID,
      options: this.#terminalOptions(),
    })
    this.#applyMetadata(response.terminal)
  }

  #terminalOptions() {
    return {
      cwd: this.cwd ?? undefined,
      dimensions: {
        cols: Math.max(this.terminal.cols, 1),
        rows: Math.max(this.terminal.rows, 1),
      },
    }
  }

  #handleEvent(event: TerminalDaemonEvent) {
    if (this.#isDisposed) {
      return
    }

    switch (event.type) {
      case "terminal.created":
        if (event.terminal.instanceId === DEFAULT_TERMINAL_ID) {
          this.#applyMetadata(event.terminal)
        }
        return
      case "terminal.output":
        if (event.instanceId === DEFAULT_TERMINAL_ID) {
          this.#appendOutput(event.data)
        }
        return
      case "terminal.exit":
        if (event.instanceId === DEFAULT_TERMINAL_ID) {
          this.status = "exited"
          this.exitCode = event.exitCode
          this.signal = event.signal
        }
        return
      case "terminal.title":
        if (event.instanceId === DEFAULT_TERMINAL_ID) {
          this.title = event.title
        }
        return
      case "terminal.cwd":
        if (event.instanceId === DEFAULT_TERMINAL_ID) {
          this.cwd = event.cwd
        }
        return
      case "terminal.error":
        if (!event.instanceId || event.instanceId === DEFAULT_TERMINAL_ID) {
          this.status = "error"
          this.statusMessage = event.message
        }
        return
    }
  }

  #appendOutput(data: string) {
    this.#nextChunkId += 1
    this.#chunks.push({
      id: `${this.terminalId}:${this.#nextChunkId}`,
      data,
    })
    this.terminal.syncChunks(this.#chunks)
  }

  #clearOutput() {
    this.#chunks = []
    this.#nextChunkId = 0
    this.terminal.syncChunks(this.#chunks)
  }

  #applyMetadata(metadata: TerminalMetadata) {
    this.cwd = metadata.cwd
    this.title = metadata.title ?? "Terminal"
    this.status = metadata.state === "closed" ? "closed" : metadata.state
    this.cols = metadata.dimensions.cols
    this.rows = metadata.dimensions.rows
    this.exitCode = metadata.exitCode ?? null
    this.signal = metadata.signal ?? null
  }

  async #disconnect() {
    const unsubscribe = this.#unsubscribe
    const connection = this.#connection
    this.#unsubscribe = null
    this.#connection = null

    unsubscribe?.()
    await connection?.disconnect().catch(() => {})
  }
}

export interface TerminalTab extends TerminalTabState {}

const terminalTabs = new Map<string, TerminalTab>()

listen(workbenchTabLifecycle, "closed", ({ tab }) => {
  if (tab.kind === "terminal") {
    disposeTerminalTab(tab.payload.terminalId)
  }
})

/** Returns the retained terminal tab owner for one workbench terminal payload. */
export function getTerminalTab(payload: TerminalTabPayload) {
  const existing = terminalTabs.get(payload.terminalId)

  if (existing) {
    return existing
  }

  const terminal = new TerminalTab(payload)
  terminalTabs.set(payload.terminalId, terminal)
  return terminal
}

/** Disposes one retained terminal tab owner after its workbench tab closes. */
export function disposeTerminalTab(terminalId: string) {
  const terminal = terminalTabs.get(terminalId)

  if (!terminal) {
    return
  }

  terminalTabs.delete(terminalId)
  terminal.dispose()
}
