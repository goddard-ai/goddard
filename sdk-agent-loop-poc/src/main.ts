#!/usr/bin/env bun
/** Minimal external-script proof of concept for running a daemon-backed SDK agent loop. */
import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import { createInterface } from "node:readline/promises"
import { parseArgs } from "node:util"
import { isCancel, text } from "@clack/prompts"
import type { AgentSession, DaemonSession } from "@goddard-ai/sdk"
import { GoddardSdk } from "@goddard-ai/sdk/node"

import { createAgentClient, createAgentLoopInterruptController, runAgentLoop } from "./loop.ts"

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
      --prompt <text>              Prompt to repeat each loop iteration. When omitted, prompt interactively.
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

/** Returns a human-readable active model label when the agent reports model state. */
function findCurrentModelLabel(session: Pick<DaemonSession, "models">) {
  const currentModelId = session.models?.currentModelId
  if (!currentModelId) {
    return null
  }

  const currentModel = session.models?.availableModels.find(
    (model) => model.modelId === currentModelId,
  )
  if (!currentModel?.name || currentModel.name === currentModelId) {
    return currentModelId
  }

  return `${currentModel.name} (${currentModelId})`
}

/** Writes daemon-resolved defaults when the caller omitted agent or model choices. */
function writeResolvedDefaults(input: {
  options: Pick<CliOptions, "agent" | "model">
  session: Pick<DaemonSession, "agentName" | "models">
  output: NodeJS.WritableStream
}) {
  if (!input.options.agent) {
    input.output.write(`Using default agent: ${input.session.agentName}\n`)
  }

  if (!input.options.model) {
    input.output.write(
      `Using default model: ${findCurrentModelLabel(input.session) ?? "not reported by agent"}\n`,
    )
  }
}

/** Creates the prompt reader used by foreground loop control. */
function createPromptReader() {
  if (process.stdin.isTTY && process.stderr.isTTY) {
    return {
      async readPrompt() {
        const value = await text({
          message: "Prompt",
          placeholder: "Type /exit to quit",
          input: process.stdin,
          output: process.stderr,
        })

        return isCancel(value) ? "/exit" : value
      },
      async readInterruptPrompt() {
        const value = await text({
          message: "Interrupted",
          placeholder: "Press Enter to resume, or type a custom prompt",
          defaultValue: "",
          input: process.stdin,
          output: process.stderr,
        })

        return isCancel(value) ? "" : value
      },
    }
  }

  const input = createInterface({
    input: process.stdin,
    output: process.stderr,
    terminal: false,
  })
  const iterator = input[Symbol.asyncIterator]()

  return {
    async readPrompt() {
      const next = await iterator.next()
      return next.done ? null : next.value
    },
    async readInterruptPrompt() {
      return ""
    },
    close() {
      input.close()
    },
  }
}

/** Installs process signal handlers that interrupt or shut down the foreground SDK session. */
function installSignalHandlers(input: {
  closeSession: () => Promise<void>
  interrupts: ReturnType<typeof createAgentLoopInterruptController>
}) {
  const handleSigint = () => {
    process.stderr.write("\n[interrupt requested]\n")
    input.interrupts.request()
  }
  const handleSigterm = () => {
    void input.closeSession().finally(() => process.exit(143))
  }

  process.on("SIGINT", handleSigint)
  process.on("SIGTERM", handleSigterm)

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
  const interrupts = createAgentLoopInterruptController()
  const removeSignalHandlers = installSignalHandlers({
    closeSession,
    interrupts,
  })

  try {
    const resolvedSession = await sdk.session.get({ id: session.sessionId })
    writeResolvedDefaults({
      options,
      session: resolvedSession.session,
      output: process.stderr,
    })

    await runAgentLoop({
      session,
      cwd: options.cwd,
      loopPrompt: options.prompt,
      cycleDelayMs: options.cycleDelayMs,
      maxIterations: options.maxIterations,
      statusOutput: process.stderr,
      closeSession,
      promptReader: createPromptReader(),
      interrupts,
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
