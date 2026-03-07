import { existsSync } from "node:fs"
import path from "node:path"

import { SessionManager, createAgentSession } from "@mariozechner/pi-coding-agent"

import type { SessionDriver, SessionDriverContext, SessionDriverInput } from "./types.ts"

async function resolveSessionManager(
  input: SessionDriverInput,
  context: SessionDriverContext,
): Promise<SessionManager> {
  if (!input.resume) {
    return SessionManager.create(context.cwd)
  }

  const candidatePath = path.resolve(context.cwd, input.resume)
  if (existsSync(candidatePath)) {
    return SessionManager.open(candidatePath)
  }

  const sessions = await SessionManager.list(context.cwd)
  const matches = sessions.filter(
    (session) => session.id === input.resume || session.id.startsWith(input.resume || ""),
  )

  if (matches.length === 1) {
    return SessionManager.open(matches[0].path)
  }

  if (matches.length > 1) {
    throw new Error(`Resume target "${input.resume}" is ambiguous. Provide a full session id.`)
  }

  throw new Error(`Unable to resolve pi session "${input.resume}"`)
}

export const driver: SessionDriver = {
  name: "pi",
  run: async (input, context) => {
    const sessionManager = await resolveSessionManager(input, context)
    const { session } = await createAgentSession({
      cwd: context.cwd,
      sessionManager,
    })

    try {
      if (input.initialPrompt) {
        await session.sendUserMessage(input.initialPrompt)
        const response = session.getLastAssistantText()
        if (response) {
          context.stdout.write(`${response}\n`)
        }
      }

      return 0
    } finally {
      session.dispose()
    }
  },
}
