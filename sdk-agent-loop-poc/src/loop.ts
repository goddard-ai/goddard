import { createInterface } from "node:readline/promises"
import type * as acp from "@agentclientprotocol/sdk"
import type { AgentSession } from "@goddard-ai/sdk"

/** Runtime inputs for the foreground prompt loop after CLI and SDK setup are complete. */
export type AgentLoopOptions = {
  session: AgentSession
  cwd: string
  initialPrompt?: string
  cycleDelayMs: number
  maxIterations?: number
  input: NodeJS.ReadableStream
  inputOutput: NodeJS.WritableStream
  statusOutput: NodeJS.WritableStream
  terminal: boolean
  closeSession: () => Promise<void>
}

/** Extracts printable assistant text from ACP session update payloads. */
function getTextChunk(params: unknown) {
  if (typeof params !== "object" || params === null || !("update" in params)) {
    return null
  }

  const update = params.update
  if (
    typeof update !== "object" ||
    update === null ||
    !("sessionUpdate" in update) ||
    update.sessionUpdate !== "agent_message_chunk" ||
    !("content" in update)
  ) {
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

/** Creates the ACP client callbacks used to stream agent text and deny tool permissions. */
export function createAgentClient(messageOutput: NodeJS.WritableStream) {
  return {
    async requestPermission() {
      return { outcome: { outcome: "cancelled" } }
    },
    async sessionUpdate(params) {
      const text = getTextChunk(params)
      if (text) {
        messageOutput.write(text)
      }
    },
  } satisfies acp.Client
}

/** Submits one user prompt and writes the turn completion marker. */
async function promptOnce(
  session: AgentSession,
  prompt: string,
  statusOutput: NodeJS.WritableStream,
) {
  const result = await session.prompt(prompt)
  statusOutput.write(`\n[stopReason: ${result.stopReason}]\n`)
}

/** Sleeps for a requested pacing interval without imposing a minimum delay. */
async function sleep(ms: number) {
  if (ms <= 0) {
    return
  }

  await new Promise((resolve) => setTimeout(resolve, ms))
}

/** Runs a live SDK session as a prompt loop owned by this external script. */
export async function runAgentLoop(options: AgentLoopOptions) {
  try {
    options.statusOutput.write(`Started session ${options.session.sessionId} in ${options.cwd}\n`)

    let iterationCount = 0
    const runIteration = async (prompt: string) => {
      if (options.maxIterations !== undefined && iterationCount >= options.maxIterations) {
        return false
      }

      if (iterationCount > 0) {
        await sleep(options.cycleDelayMs)
      }

      iterationCount += 1
      await promptOnce(options.session, prompt, options.statusOutput)
      return options.maxIterations === undefined || iterationCount < options.maxIterations
    }

    if (options.initialPrompt) {
      const canContinue = await runIteration(options.initialPrompt)
      if (!canContinue) {
        return
      }
    }

    const input = createInterface({
      input: options.input,
      output: options.inputOutput,
      terminal: options.terminal,
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
    await options.closeSession()
  }
}
