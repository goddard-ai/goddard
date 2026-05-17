#!/usr/bin/env bun
/** Minimal external-script proof of concept for running a daemon-backed SDK agent loop. */
import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import { parseArgs } from "node:util"
import type { AgentSession } from "@goddard-ai/sdk"
import { GoddardSdk } from "@goddard-ai/sdk/node"

import { createAgentClient, runAgentLoop } from "./loop.ts"

/** Parsed command-line options before the entrypoint resolves files and SDK objects. */
type CliOptions = {
  systemPromptFile: string
  cwd: string
  agent?: string
  model?: string
  prompt?: string
  daemonUrl?: string
  cycleDelayMs: number
  maxIterations?: number
}

/** Builds the CLI help text shown for --help and validation errors. */
function usage() {
  return `Usage:
  sdk-agent-loop-poc --system-prompt-file ./prompt.md [options]

Options:
  -s, --system-prompt-file <path>  System prompt file to load.
      --cwd <path>                 Agent working directory. Defaults to process.cwd().
      --agent <name>               Optional ACP adapter name or distribution id.
      --model <model-id>           Optional initial model id.
      --prompt <text>              Optional first prompt before reading stdin.
      --cycle-delay <duration>     Delay between prompt iterations. Supports ms, s, m, h, d. Defaults to 0s.
      --max-iterations <count>     Maximum prompts to send before exiting. Defaults to unlimited.
      --daemon-url <url>           Optional daemon URL override.
      --help                       Show this help text.
`
}

/** Parses compact CLI duration values into millisecond delays. */
function parseDurationMs(value: string) {
  const match = value.match(/^(\d+)(ms|s|m|h|d)?$/)
  if (!match) {
    throw new Error(`Invalid --cycle-delay value "${value}". Use values like 500ms, 5s, or 1m.`)
  }

  const amount = Number.parseInt(match[1] ?? "0", 10)
  const unit = match[2] ?? "ms"

  switch (unit) {
    case "ms":
      return amount
    case "s":
      return amount * 1000
    case "m":
      return amount * 60 * 1000
    case "h":
      return amount * 60 * 60 * 1000
    case "d":
      return amount * 24 * 60 * 60 * 1000
  }

  throw new Error(`Invalid --cycle-delay unit "${unit}".`)
}

/** Parses positive integer CLI limits without accepting partial numeric strings. */
function parsePositiveInteger(name: string, value: string) {
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || String(parsed) !== value || parsed <= 0) {
    throw new Error(`Invalid ${name} value "${value}". Use a positive whole number.`)
  }

  return parsed
}

/** Converts process argv into validated runtime options. */
function readCliOptions() {
  const parsed = parseArgs({
    args: process.argv.slice(2),
    options: {
      "system-prompt-file": {
        type: "string",
        short: "s",
      },
      cwd: {
        type: "string",
      },
      agent: {
        type: "string",
      },
      model: {
        type: "string",
      },
      prompt: {
        type: "string",
      },
      "cycle-delay": {
        type: "string",
      },
      "max-iterations": {
        type: "string",
      },
      "daemon-url": {
        type: "string",
      },
      help: {
        type: "boolean",
      },
    },
  })

  if (parsed.values.help) {
    process.stdout.write(usage())
    process.exit(0)
  }

  const systemPromptFile = parsed.values["system-prompt-file"]
  if (!systemPromptFile) {
    throw new Error("Missing required --system-prompt-file option.")
  }

  const options: CliOptions = {
    systemPromptFile,
    cwd: resolve(parsed.values.cwd ?? process.cwd()),
    agent: parsed.values.agent,
    model: parsed.values.model,
    prompt: parsed.values.prompt,
    daemonUrl: parsed.values["daemon-url"],
    cycleDelayMs: parseDurationMs(parsed.values["cycle-delay"] ?? "0s"),
    maxIterations: parsed.values["max-iterations"]
      ? parsePositiveInteger("--max-iterations", parsed.values["max-iterations"])
      : undefined,
  }

  return options
}

/** Creates one idempotent close function shared by signal handling and normal shutdown. */
function createCloseSession(session: AgentSession | null) {
  if (!session) {
    throw new Error("Interactive prompts require a live session; do not create it with oneShot.")
  }

  let closed = false

  return {
    session,
    async closeSession() {
      if (closed) {
        return
      }

      closed = true
      await session.stop()
    },
  }
}

/** Installs process signal handlers that shut down the foreground SDK session. */
function installSignalHandlers(closeSession: () => Promise<void>) {
  const handleSigint = () => {
    void closeSession().finally(() => process.exit(130))
  }
  const handleSigterm = () => {
    void closeSession().finally(() => process.exit(143))
  }

  process.once("SIGINT", handleSigint)
  process.once("SIGTERM", handleSigterm)

  return () => {
    process.off("SIGINT", handleSigint)
    process.off("SIGTERM", handleSigterm)
  }
}

async function main() {
  const options = readCliOptions()
  const systemPrompt = await readFile(resolve(options.systemPromptFile), "utf8")
  const sdk = new GoddardSdk(
    options.daemonUrl
      ? {
          daemonUrl: options.daemonUrl,
        }
      : {},
  )
  const { session, closeSession } = createCloseSession(
    await sdk.session.run(
      {
        agent: options.agent,
        cwd: options.cwd,
        mcpServers: [],
        systemPrompt,
        initialModelId: options.model,
      },
      createAgentClient(process.stdout),
    ),
  )
  const removeSignalHandlers = installSignalHandlers(closeSession)

  try {
    await runAgentLoop({
      session,
      cwd: options.cwd,
      initialPrompt: options.prompt,
      cycleDelayMs: options.cycleDelayMs,
      maxIterations: options.maxIterations,
      input: process.stdin,
      inputOutput: process.stderr,
      statusOutput: process.stderr,
      terminal: process.stdin.isTTY,
      closeSession,
    })
  } finally {
    removeSignalHandlers()
  }
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`${message}\n\n${usage()}`)
  process.exit(1)
})
