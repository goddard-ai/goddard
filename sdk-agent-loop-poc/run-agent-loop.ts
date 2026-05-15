#!/usr/bin/env bun
/** Minimal external-script proof of concept for running a daemon-backed SDK agent loop. */
import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import { createInterface } from "node:readline/promises"
import { parseArgs } from "node:util"
import type * as acp from "@agentclientprotocol/sdk"
import type { AgentSession } from "@goddard-ai/sdk"
import { GoddardSdk } from "@goddard-ai/sdk/node"

type CliOptions = {
  systemPromptFile: string
  cwd: string
  agent?: string
  model?: string
  prompt?: string
  daemonUrl?: string
}

function usage() {
  return `Usage:
  bun run ./sdk-agent-loop-poc/run-agent-loop.ts --system-prompt-file ./prompt.md [options]

Options:
  -s, --system-prompt-file <path>  System prompt file to load.
      --cwd <path>                 Agent working directory. Defaults to process.cwd().
      --agent <name>               Optional ACP adapter name or distribution id.
      --model <model-id>           Optional initial model id.
      --prompt <text>              Optional first prompt before reading stdin.
      --daemon-url <url>           Optional daemon URL override.
      --help                       Show this help text.
`
}

function readCliOptions(): CliOptions {
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
  }
}

function getTextChunk(params: unknown) {
  if (typeof params !== "object" || params === null || !("update" in params)) {
    return null
  }

  const update = params.update
  if (typeof update !== "object" || update === null || !("content" in update)) {
    return null
  }

  const content = update.content
  if (
    typeof content === "object" &&
    content !== null &&
    "type" in content &&
    content.type === "text" &&
    "text" in content &&
    typeof content.text === "string"
  ) {
    return content.text
  }

  return null
}

function createAgentClient(): acp.Client {
  return {
    async requestPermission() {
      return { outcome: { outcome: "cancelled" } }
    },
    async sessionUpdate(params) {
      const text = getTextChunk(params)
      if (text) {
        process.stdout.write(text)
      }
    },
  }
}

async function promptOnce(session: AgentSession, prompt: string) {
  const result = await session.prompt(prompt)
  process.stdout.write(`\n[stopReason: ${result.stopReason}]\n`)
}

async function runPromptLoop(options: CliOptions) {
  const systemPrompt = await readFile(resolve(options.systemPromptFile), "utf8")
  const sdk = new GoddardSdk(
    options.daemonUrl
      ? {
          daemonUrl: options.daemonUrl,
        }
      : {},
  )

  const session = await sdk.session.run(
    {
      agent: options.agent,
      cwd: options.cwd,
      mcpServers: [],
      systemPrompt,
      initialModelId: options.model,
    },
    createAgentClient(),
  )
  if (!session) {
    throw new Error("Interactive prompts require a live session; do not create it with oneShot.")
  }

  let stopped = false
  const stop = async () => {
    if (stopped) {
      return
    }

    stopped = true
    await session.stop()
  }

  process.on("SIGINT", () => {
    void stop().finally(() => process.exit(130))
  })
  process.on("SIGTERM", () => {
    void stop().finally(() => process.exit(143))
  })

  try {
    process.stderr.write(`Started session ${session.sessionId} in ${options.cwd}\n`)

    if (options.prompt) {
      await promptOnce(session, options.prompt)
    }

    if (!process.stdin.isTTY && options.prompt) {
      return
    }

    const input = createInterface({
      input: process.stdin,
      output: process.stderr,
      terminal: process.stdin.isTTY,
    })

    for await (const line of input) {
      const prompt = line.trim()
      if (!prompt) {
        continue
      }

      if (prompt === "/exit" || prompt === "/quit") {
        break
      }

      await promptOnce(session, prompt)
    }
  } finally {
    await stop()
  }
}

async function main() {
  await runPromptLoop(readCliOptions())
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`${message}\n\n${usage()}`)
  process.exit(1)
})
