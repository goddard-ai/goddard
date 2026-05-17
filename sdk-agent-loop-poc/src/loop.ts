import type * as acp from "@agentclientprotocol/sdk"
import type { AgentSession } from "@goddard-ai/sdk"
import { bold, cyan, dim, green, red, yellow } from "nanocolors"

/** Interrupt signal shared between process handlers, sleeps, and active prompt cancellation. */
export type AgentLoopInterruptController = {
  request: () => void
  peek: () => boolean
  consume: () => boolean
  onInterrupt: (listener: () => void) => () => void
}

/** User prompt adapter used by the loop without binding it to a specific terminal UI. */
export type AgentLoopPromptReader = {
  readPrompt: () => Promise<string | null>
  readInterruptPrompt: () => Promise<string | null>
  close?: () => void
}

/** Runtime inputs for the foreground prompt loop after CLI and SDK setup are complete. */
export type AgentLoopOptions = {
  session: AgentSession
  cwd: string
  loopPrompt?: string
  cycleDelayMs: number
  maxIterations?: number
  statusOutput: NodeJS.WritableStream
  closeSession: () => Promise<void>
  promptReader: AgentLoopPromptReader
  interrupts?: AgentLoopInterruptController
}

/** Creates one in-memory interrupt controller for a foreground loop process. */
export function createAgentLoopInterruptController() {
  const listeners = new Set<() => void>()
  let requested = false

  return {
    request() {
      requested = true

      for (const listener of listeners) {
        listener()
      }
    },
    peek() {
      return requested
    },
    consume() {
      const wasRequested = requested
      requested = false
      return wasRequested
    },
    onInterrupt(listener: () => void) {
      listeners.add(listener)

      return () => {
        listeners.delete(listener)
      }
    },
  } satisfies AgentLoopInterruptController
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

/** Writes a muted foreground-loop status line to stderr-like output. */
function writeStatus(output: NodeJS.WritableStream, message: string) {
  output.write(`${dim("›")} ${message}\n`)
}

/** Writes the user prompt being sent for this foreground loop iteration. */
function writeLoopPrompt(output: NodeJS.WritableStream, prompt: string) {
  output.write(`\n${bold(green("Loop prompt"))}\n\n${green(prompt)}\n`)
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
        messageOutput.write(cyan(text))
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
  writeLoopPrompt(statusOutput, prompt)
  statusOutput.write(`\n${bold(cyan("Agent response"))}\n\n`)
  const result = await session.prompt(prompt)
  statusOutput.write(`\n\n${dim(`[stopReason: ${result.stopReason}]`)}\n\n`)
}

/** Sleeps for a requested pacing interval without imposing a minimum delay. */
async function sleep(ms: number) {
  if (ms <= 0) {
    return
  }

  await new Promise((resolve) => setTimeout(resolve, ms))
}

/** Sleeps for loop pacing unless an interrupt asks the loop to resume control early. */
async function sleepUntilDelayOrInterrupt(ms: number, interrupts?: AgentLoopInterruptController) {
  if (ms <= 0) {
    return true
  }

  if (!interrupts) {
    await sleep(ms)
    return true
  }

  if (interrupts.peek()) {
    return false
  }

  return await new Promise<boolean>((resolve) => {
    const timeout = setTimeout(() => {
      unsubscribe()
      resolve(true)
    }, ms)
    const unsubscribe = interrupts.onInterrupt(() => {
      clearTimeout(timeout)
      unsubscribe()
      resolve(false)
    })
  })
}

/** Recognizes local control commands that terminate the foreground loop. */
function isExitCommand(prompt: string) {
  return prompt === "/exit" || prompt === "/quit"
}

/** Reads the optional custom prompt requested after an interrupt. */
async function readInterruptPrompt(options: AgentLoopOptions) {
  if (!options.interrupts?.consume()) {
    return null
  }

  const line = await options.promptReader.readInterruptPrompt()
  options.interrupts?.consume()

  const prompt = line?.trim() ?? ""

  if (isExitCommand(prompt)) {
    return { kind: "exit" as const }
  }

  if (!prompt) {
    return { kind: "resume" as const }
  }

  return {
    kind: "prompt" as const,
    prompt,
  }
}

/** Runs a live SDK session as a prompt loop owned by this external script. */
export async function runAgentLoop(options: AgentLoopOptions) {
  let promptInProgress = false
  const removeInterruptHandler = options.interrupts?.onInterrupt(() => {
    if (!promptInProgress) {
      return
    }

    void options.session.cancel().catch((error: unknown) => {
      const message = error instanceof Error ? error.message : String(error)
      options.statusOutput.write(`\n${red(`interrupt cancel failed: ${message}`)}\n\n`)
    })
  })

  try {
    writeStatus(options.statusOutput, `${dim("started session")} ${options.session.sessionId}`)
    writeStatus(options.statusOutput, `${dim("cwd")} ${options.cwd}`)
    options.statusOutput.write("\n")

    let iterationCount = 0
    let skipDelay = false
    const runIteration = async (prompt: string) => {
      if (options.maxIterations !== undefined && iterationCount >= options.maxIterations) {
        return false
      }

      if (iterationCount > 0 && !skipDelay) {
        const completedDelay = await sleepUntilDelayOrInterrupt(
          options.cycleDelayMs,
          options.interrupts,
        )
        if (!completedDelay) {
          skipDelay = true
          return true
        }
      }

      skipDelay = false
      iterationCount += 1
      promptInProgress = true
      try {
        await promptOnce(options.session, prompt, options.statusOutput)
      } catch (error) {
        if (!options.interrupts?.peek()) {
          throw error
        }

        options.statusOutput.write(`\n${yellow("interrupted")}\n\n`)
      } finally {
        promptInProgress = false
      }
      return options.maxIterations === undefined || iterationCount < options.maxIterations
    }

    while (true) {
      const interruptPrompt = await readInterruptPrompt(options)
      if (interruptPrompt?.kind === "exit") {
        break
      }

      const line =
        interruptPrompt?.kind === "prompt"
          ? interruptPrompt.prompt
          : (options.loopPrompt ?? (await options.promptReader.readPrompt()))

      if (line === null) {
        return
      }

      const prompt = line.trim()
      if (!prompt) {
        continue
      }
      if (isExitCommand(prompt)) {
        return
      }

      const canContinue = await runIteration(prompt)
      if (!canContinue) {
        break
      }
    }
  } finally {
    removeInterruptHandler?.()
    options.promptReader.close?.()
    await options.closeSession()
  }
}
