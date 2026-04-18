#!/usr/bin/env bun
/** ACP debugging CLI for daemon-backed sessions and raw adapters. */
import { cancel, intro, isCancel, log, note, outro, text } from "@clack/prompts"
import { command, option, optional, positional, run, string, subcommands } from "cmd-ts"

import { inspectAdapterSession } from "../core/daemon/src/session/inspect.ts"
import { ACPAdapterNames } from "../core/schema/src/acp-adapters.ts"
import { GoddardSdk } from "../core/sdk/src/node/index.ts"
import {
  createSessionPromptMessage,
  type DaemonSession,
  type GetSessionHistoryResponse,
  type SessionHistoryTurn,
} from "../core/sdk/src/session.ts"

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

/** Flattens paged turn history into the raw `session/update` notifications this CLI prints. */
export function collectSessionUpdates(turns: SessionHistoryTurn[]) {
  return turns.flatMap((turn) => turn.messages).filter(isSessionUpdateMessage)
}

/** Returns the serialized resume payload used when interactive attach is unavailable. */
function buildResumeSnapshot(
  session: DaemonSession,
  history: GetSessionHistoryResponse,
  resumed: boolean,
) {
  return {
    resumed,
    session,
    connection: history.connection,
    turns: history.turns,
    nextCursor: history.nextCursor,
    hasMore: history.hasMore,
    sessionUpdates: collectSessionUpdates(history.turns),
  }
}

/** Matches one JSON-RPC result or error payload for the specified prompt request id. */
export function getPromptCompletionMessage(message: unknown, requestId: string | number) {
  if (
    typeof message !== "object" ||
    message === null ||
    "id" in message === false ||
    message.id !== requestId
  ) {
    return null
  }

  if ("result" in message) {
    return {
      kind: "result" as const,
      message,
    }
  }

  if ("error" in message) {
    return {
      kind: "error" as const,
      message,
    }
  }

  return null
}

/** Returns true when one REPL command should detach from the resumed live session. */
export function isResumeExitCommand(input: string) {
  const normalized = input.trim().toLowerCase()
  return normalized === "/exit" || normalized === "/quit"
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
      resolvedAgent ? ` with agent ${resolvedAgent}` : " using the daemon-resolved default agent"
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

/** Reconnects to one daemon session and attaches an interactive prompt loop when possible. */
async function resumeSession(id: string, initialPrompt?: string) {
  const sdk = new GoddardSdk()
  const [{ session }, history] = await Promise.all([
    sdk.session.get({ id }),
    sdk.session.history({ id }),
  ])
  const resumeSnapshot = buildResumeSnapshot(session, history, false)

  if (!history.connection.reconnectable) {
    if (history.connection.mode === "history") {
      cancel(`Session ${id} is archived and no longer reconnectable.`)
    } else {
      cancel(`Session ${id} is not reconnectable.`)
    }

    writeJson(resumeSnapshot)
    return
  }

  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new Error("`acp resume` requires an interactive TTY when the session is reconnectable")
  }

  await sdk.session.connect({ id })
  intro(`Resuming ${id}`)
  note(
    [
      `agent: ${session.agentName}`,
      `cwd: ${session.cwd}`,
      `turns loaded: ${history.turns.length}`,
      "session messages stream below as NDJSON.",
      "type /exit to detach from the session.",
    ].join("\n"),
    "ACP REPL",
  )

  const pendingPrompts = new Map<
    string | number,
    {
      resolve: () => void
      reject: (error: Error) => void
    }
  >()
  const unsubscribe = await sdk.session.subscribe({ id }, (message) => {
    logStreamMessage(message)

    if (typeof message !== "object" || message === null || "id" in message === false) {
      return
    }

    const promptId = message.id as string | number
    const pendingPrompt = pendingPrompts.get(promptId)
    if (!pendingPrompt) {
      return
    }

    const completion = getPromptCompletionMessage(message, promptId)
    if (!completion) {
      return
    }

    pendingPrompts.delete(promptId)
    if (completion.kind === "error") {
      pendingPrompt.reject(new Error(JSON.stringify(completion.message)))
      return
    }

    pendingPrompt.resolve()
  })

  /** Queues one prompt into the resumed session and waits for its matching JSON-RPC completion. */
  const sendPrompt = async (prompt: string) => {
    const message = createSessionPromptMessage({
      id,
      acpId: session.acpSessionId,
      prompt,
    })
    const requestId = message.id
    if (requestId == null) {
      throw new Error("Prompt requests must include a JSON-RPC id")
    }

    const completion = new Promise<void>((resolve, reject) => {
      pendingPrompts.set(requestId, { resolve, reject })
    })

    try {
      await sdk.session.send({
        id,
        message,
      })
    } catch (error) {
      pendingPrompts.delete(requestId)
      throw error
    }

    await completion
  }

  try {
    const firstPrompt = initialPrompt?.trim()
    if (firstPrompt) {
      await sendPrompt(firstPrompt)
    }

    while (true) {
      const nextPrompt = await text({
        message: "Prompt",
        placeholder: "Type a prompt or /exit",
      })
      if (isCancel(nextPrompt)) {
        cancel(`Detached from session ${id}.`)
        return
      }
      if (typeof nextPrompt !== "string") {
        continue
      }

      const trimmedPrompt = nextPrompt.trim()
      if (!trimmedPrompt) {
        continue
      }
      if (isResumeExitCommand(trimmedPrompt)) {
        outro(`Detached from session ${id}.`)
        return
      }

      try {
        await sendPrompt(trimmedPrompt)
      } catch (error) {
        log.error(error instanceof Error ? error.message : String(error))
      }
    }
  } finally {
    unsubscribe()
  }
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
      description: "Resume one daemon session and attach an interactive ACP prompt loop",
      args: {
        id: positional({
          type: string,
          displayName: "id",
          description: "Daemon session id to inspect",
        }),
        prompt: positional({
          type: optional(string),
          displayName: "prompt",
          description: "Optional first prompt to send before entering the REPL",
        }),
      },
      handler: async ({ id, prompt }) => {
        await resumeSession(id.trim(), prompt)
      },
    }),
  },
})

/** Parses argv and runs the ACP debugging CLI. */
async function main(argv = process.argv.slice(2)) {
  await run(app, argv)
}

if (import.meta.main) {
  await main().catch((error) => {
    process.stderr.write(
      `[acp] ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
    )
    process.exit(1)
  })
}
