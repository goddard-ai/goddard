import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import { createInterface } from "node:readline/promises"
import type * as acp from "@agentclientprotocol/sdk"
import type { AgentSession } from "@goddard-ai/sdk"
import { GoddardSdk } from "@goddard-ai/sdk/node"

export type AgentLoopOptions = {
  systemPromptFile: string
  cwd: string
  agent?: string
  model?: string
  prompt?: string
  daemonUrl?: string
  cycleDelayMs: number
  maxIterations?: number
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

async function sleep(ms: number) {
  if (ms <= 0) {
    return
  }

  await new Promise((resolve) => setTimeout(resolve, ms))
}

/** Runs a live SDK session as a prompt loop owned by this external script. */
export async function runAgentLoop(options: AgentLoopOptions) {
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

    let iterationCount = 0
    const runIteration = async (prompt: string) => {
      if (options.maxIterations !== undefined && iterationCount >= options.maxIterations) {
        return false
      }

      if (iterationCount > 0) {
        await sleep(options.cycleDelayMs)
      }

      iterationCount += 1
      await promptOnce(session, prompt)
      return options.maxIterations === undefined || iterationCount < options.maxIterations
    }

    if (options.prompt) {
      const canContinue = await runIteration(options.prompt)
      if (!canContinue) {
        return
      }
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

      const canContinue = await runIteration(prompt)
      if (!canContinue) {
        break
      }
    }
  } finally {
    await stop()
  }
}
