/** Daemon terminal PTY smoke check used by tests and standalone diagnostics. */
import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon/terminals"

import { DaemonTerminalConnection } from "./runtime.ts"

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

/** Runs spawn, write, resize, and close through one daemon terminal connection. */
export async function runTerminalRuntimeCheck() {
  const events: TerminalDaemonEvent[] = []
  const connection = new DaemonTerminalConnection({
    connectionId: "terminal-runtime-check-connection",
    onEvent: (event) => {
      events.push(event)
    },
  })
  const connectionId = "terminal-runtime-check-connection"
  const instanceId = "terminal-runtime-check"

  try {
    connection.create({
      connectionId,
      instanceId,
      options: {
        command: resolveCheckShell(),
        dimensions: {
          cols: 80,
          rows: 24,
        },
      },
    })
    connection.resize({
      connectionId,
      instanceId,
      dimensions: {
        cols: 100,
        rows: 30,
      },
    })
    connection.write({
      connectionId,
      instanceId,
      data: buildMarkerCommand(),
    })

    await waitFor(() => collectOutput(events).includes(MARKER), DEFAULT_TIMEOUT_MS)
    connection.close({
      connectionId,
      instanceId,
    })

    return buildResult(events, connection.size)
  } finally {
    connection.closeAll()
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
  return process.platform === "win32" ? (process.env.COMSPEC ?? "cmd.exe") : "/bin/sh"
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
