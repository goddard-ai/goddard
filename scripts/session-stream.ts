#!/usr/bin/env bun
import { command, option, optional, positional, run, string } from "cmd-ts"
import { GoddardSdk } from "../core/sdk/src/node/index.ts"

const DEFAULT_PROMPT =
  "Reply in two short sentences so I can verify the session update stream is working."

/** Resolves the prompt from the positional argument or falls back to the default text. */
function resolvePrompt(prompt?: string): string {
  const trimmedPrompt = prompt?.trim() ?? ""
  return trimmedPrompt || DEFAULT_PROMPT
}

/** Writes one streamed ACP message as a single JSON line. */
function logStreamMessage(message: unknown) {
  process.stdout.write(`${JSON.stringify(message)}\n`)
}

/** Installs signal handlers that stop the session before the process exits. */
function installSignalHandlers(stopSession: () => Promise<void>) {
  const handleSignal = (signal: NodeJS.Signals) => {
    process.stderr.write(`[session:stream] received ${signal}, shutting down\n`)
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

/** Starts a daemon-backed SDK session, subscribes to its stream, and sends one prompt. */
async function runSessionStream(input: { agent?: string; prompt?: string }) {
  const prompt = resolvePrompt(input.prompt)
  const agent = input.agent?.trim() || undefined
  const sdk = new GoddardSdk()
  const session = await sdk.session.run({
    agent,
    cwd: process.cwd(),
    mcpServers: [],
  })

  process.stderr.write(
    `[session:stream] started session ${session.sessionId}${agent ? ` with agent ${agent}` : ""}\n`,
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

  const removeSignalHandlers = installSignalHandlers(stopSession)

  try {
    const result = await session.prompt(prompt)
    process.stderr.write(
      `[session:stream] prompt completed with stop reason ${result.stopReason}\n`,
    )
  } finally {
    removeSignalHandlers()
    await stopSession()
  }
}

const app = command({
  name: "session-stream",
  description: "Start a daemon-backed agent session and print streamed ACP messages",
  args: {
    agent: option({
      type: optional(string),
      long: "agent",
      short: "A",
      description: "Agent id to start instead of using the daemon default",
    }),
    prompt: positional({
      type: optional(string),
      displayName: "prompt",
      description: "Prompt to send after the session starts",
    }),
  },
  handler: async ({ agent, prompt }) => {
    await runSessionStream({ agent, prompt })
  },
})

await run(app, process.argv.slice(2)).catch((error) => {
  process.stderr.write(
    `[session:stream] ${error instanceof Error ? error.stack ?? error.message : String(error)}\n`,
  )
  process.exit(1)
})
