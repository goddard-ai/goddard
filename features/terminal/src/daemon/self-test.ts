/** Daemon terminal PTY smoke check used by tests and standalone diagnostics. */
import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon"

import { DaemonTerminalManager } from "./runtime.ts"

const DEFAULT_TIMEOUT_MS = 5_000
const MARKER = "__goddard_terminal_runtime_ok__"

/** Result returned by the daemon terminal runtime smoke check. */
export type TerminalRuntimeCheckResult = {
  ok: boolean
  marker: string
  output: string
  events: TerminalDaemonEvent["type"][]
  remainingTerminals: number
}

/** Runs spawn, write, resize, and close through the daemon terminal manager. */
export async function runTerminalRuntimeCheck() {
  const events: TerminalDaemonEvent[] = []
  const manager = new DaemonTerminalManager({
    onEvent: (event) => {
      events.push(event)
    },
  })
  const instanceId = "terminal-runtime-check"

  try {
    manager.create({
      type: "terminal.create",
      instanceId,
      options: {
        command: resolveCheckShell(),
        dimensions: {
          cols: 80,
          rows: 24,
        },
      },
    })
    manager.resize({
      type: "terminal.resize",
      instanceId,
      dimensions: {
        cols: 100,
        rows: 30,
      },
    })
    manager.write({
      type: "terminal.input",
      instanceId,
      data: buildMarkerCommand(),
    })

    await waitFor(() => collectOutput(events).includes(MARKER), DEFAULT_TIMEOUT_MS)
    manager.close({
      type: "terminal.close",
      instanceId,
    })

    return buildResult(events, manager.size)
  } finally {
    manager.closeAll()
  }
}

function buildResult(events: TerminalDaemonEvent[], remainingTerminals: number) {
  const output = collectOutput(events)
  return {
    ok:
      output.includes(MARKER) &&
      events.some((event) => event.type === "terminal.created") &&
      events.some((event) => event.type === "terminal.exit") &&
      remainingTerminals === 0,
    marker: MARKER,
    output,
    events: events.map((event) => event.type),
    remainingTerminals,
  } satisfies TerminalRuntimeCheckResult
}

function collectOutput(events: TerminalDaemonEvent[]) {
  return events.flatMap((event) => (event.type === "terminal.output" ? [event.data] : [])).join("")
}

function resolveCheckShell() {
  return process.platform === "win32" ? "cmd.exe" : "/bin/sh"
}

function buildMarkerCommand() {
  return process.platform === "win32" ? `echo ${MARKER}\r\n` : `printf '${MARKER}\\n'\n`
}

async function waitFor(check: () => boolean, timeoutMs: number) {
  const startedAt = Date.now()
  while (!check()) {
    if (Date.now() - startedAt > timeoutMs) {
      throw new Error("Timed out waiting for daemon terminal PTY output.")
    }
    await Bun.sleep(10)
  }
}
