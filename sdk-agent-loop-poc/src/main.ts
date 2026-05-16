#!/usr/bin/env bun
/** Minimal external-script proof of concept for running a daemon-backed SDK agent loop. */
import { resolve } from "node:path"
import { parseArgs } from "node:util"

import { runAgentLoop, type AgentLoopOptions } from "./loop.ts"

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

function parsePositiveInteger(name: string, value: string) {
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || String(parsed) !== value || parsed <= 0) {
    throw new Error(`Invalid ${name} value "${value}". Use a positive whole number.`)
  }

  return parsed
}

function readCliOptions(): AgentLoopOptions {
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

  return {
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
}

async function main() {
  await runAgentLoop(readCliOptions())
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`${message}\n\n${usage()}`)
  process.exit(1)
})
