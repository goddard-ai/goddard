#!/usr/bin/env bun
import { command, option, optional, positional, run, string, subcommands } from "cmd-ts"
import { inspectAdapterSession } from "../core/daemon/src/session/inspect.ts"
import { ACPAdapterNames } from "../core/schema/src/acp-adapters.ts"
import { GoddardSdk } from "../core/sdk/src/node/index.ts"

const DEFAULT_PROMPT =
  "Reply in two short sentences so I can verify the session update stream is working."

/** Resolves the prompt from the positional argument or falls back to the default text. */
function resolvePrompt(prompt?: string) {
  const trimmedPrompt = prompt?.trim() ?? ""
  return trimmedPrompt || DEFAULT_PROMPT
}

/** Serializes one JSON payload with stable indentation so the CLI stays pipe-friendly. */
function writeJson(value: unknown) {
  process.stdout.write(`${JSON.stringify(value, null, 2)}\n`)
}

/** Writes one streamed ACP message as a single JSON line. */
function logStreamMessage(message: unknown) {
  process.stdout.write(`${JSON.stringify(message)}\n`)
}

/** Detects ACP `session/update` notifications inside daemon session history. */
function isSessionUpdateMessage(message: unknown) {
  return (
    typeof message === "object" &&
    message !== null &&
    "method" in message &&
    typeof message.method === "string" &&
    message.method === "session/update"
  )
}

/** Installs signal handlers that stop the session before the process exits. */
function installSignalHandlers(label: string, stopSession: () => Promise<void>) {
  const handleSignal = (signal: NodeJS.Signals) => {
    process.stderr.write(`[acp:${label}] received ${signal}, shutting down\n`)
    void stopSession().finally(() => {
      process.exit(130)
    })
  }

  process.on("SIGINT", handleSignal)
  process.on("SIGTERM", handleSignal)

  return () => {
    process.off("SIGINT", handleSignal)
    process.off("SIGTERM", handleSignal)
  }
}

/** Starts a daemon-backed live session and prints each streamed ACP message as NDJSON. */
async function streamSession(args: { agent?: string; model?: string; prompt?: string }) {
  const resolvedPrompt = resolvePrompt(args.prompt)
  const resolvedAgent = args.agent?.trim() || undefined
  const resolvedModel = args.model?.trim() || undefined
  const sdk = new GoddardSdk()
  const session = await sdk.session.run({
    agent: resolvedAgent,
    cwd: process.cwd(),
    mcpServers: [],
  })

  process.stderr.write(
    `[acp:stream] started session ${session.sessionId}${
      resolvedAgent ? ` with agent ${resolvedAgent}` : ""
    }\n`,
  )

  const unsubscribe = await sdk.session.subscribe({ id: session.sessionId }, (message) => {
    logStreamMessage(message)
  })

  let stopped = false

  /** Stops the daemon session once even if multiple shutdown paths race. */
  const stopSession = async () => {
    if (stopped) {
      return
    }

    stopped = true
    unsubscribe()
    await session.stop()
  }

  const removeSignalHandlers = installSignalHandlers("stream", stopSession)

  try {
    if (resolvedModel) {
      await session.setAgentModel(resolvedModel)
      process.stderr.write(`[acp:stream] set session model to ${resolvedModel}\n`)
    }

    const result = await session.prompt(resolvedPrompt)
    process.stderr.write(`[acp:stream] prompt completed with stop reason ${result.stopReason}\n`)
  } finally {
    removeSignalHandlers()
    await stopSession()
  }
}

/** Opens one raw adapter session and prints the negotiated ACP surface. */
async function showAdapter(adapter: string) {
  const inspection = await inspectAdapterSession(adapter, process.cwd())

  try {
    writeJson({
      adapter,
      cwd: process.cwd(),
      initialize: inspection.initialize,
      session: inspection.session,
      sessionUpdates: inspection.sessionUpdates,
    })
  } finally {
    inspection.close()
  }
}

/** Reconnects to one daemon session when possible and prints its stored session updates. */
async function resumeSession(id: string) {
  const sdk = new GoddardSdk()
  const { session } = await sdk.session.get({
    id,
  })

  let resumed = false
  if (session.connection.reconnectable) {
    await sdk.session.connect({ id })
    resumed = true
  } else if (session.connection.historyAvailable) {
    process.stderr.write(
      `[acp:resume] session ${id} is archived and no longer reconnectable; printing stored history\n`,
    )
  }

  const history = await sdk.session.history({ id })

  writeJson({
    resumed,
    session,
    connection: history.connection,
    sessionUpdates: history.history.filter(isSessionUpdateMessage),
  })
}

/** Prints the generated ACP adapter ids accepted by daemon-backed session creation. */
function listAgents() {
  writeJson({
    agents: ACPAdapterNames,
  })
}

const app = subcommands({
  name: "acp",
  description: "ACP debugging commands for daemon-backed sessions and raw adapters",
  cmds: {
    "list-agents": command({
      name: "list-agents",
      description: "List the available ACP adapter ids accepted as named agents",
      args: {},
      handler: listAgents,
    }),
    stream: command({
      name: "stream",
      description: "Start a daemon-backed agent session and print streamed ACP messages",
      args: {
        agent: option({
          type: optional(string),
          long: "agent",
          short: "A",
          description: "Agent id to start instead of using the daemon default",
        }),
        model: option({
          type: optional(string),
          long: "model",
          short: "m",
          description: "Model id to apply to the session before sending the prompt",
        }),
        prompt: positional({
          type: optional(string),
          displayName: "prompt",
          description: "Prompt to send after the session starts",
        }),
      },
      handler: streamSession,
    }),
    show: command({
      name: "show",
      description: "Initialize one ACP adapter directly and print its negotiated session surface",
      args: {
        adapter: positional({
          type: string,
          displayName: "adapter",
          description: "ACP adapter name to inspect",
        }),
      },
      handler: async ({ adapter }) => {
        await showAdapter(adapter.trim())
      },
    }),
    resume: command({
      name: "resume",
      description: "Resume one daemon session when possible and print its session updates",
      args: {
        id: positional({
          type: string,
          displayName: "id",
          description: "Daemon session id to inspect",
        }),
      },
      handler: async ({ id }) => {
        await resumeSession(id.trim())
      },
    }),
  },
})

await run(app, process.argv.slice(2)).catch((error) => {
  process.stderr.write(
    `[acp] ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  )
  process.exit(1)
})
