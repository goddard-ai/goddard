#!/usr/bin/env node
import { SessionStorage } from "@goddard-ai/storage"
import { command, option, run, string, subcommands } from "cmd-ts"

async function requireSessionId(): Promise<string> {
  const serverId = process.env.GODDARD_SERVER_ID
  if (!serverId) {
    throw new Error("GODDARD_SERVER_ID is required")
  }

  const session = await SessionStorage.getByServerId(serverId)
  if (!session) {
    throw new Error(`No session found for GODDARD_SERVER_ID=${serverId}`)
  }

  return session.id
}

export async function declareInitiative(sessionId: string, title: string) {
  await SessionStorage.update(sessionId, {
    initiative: title,
    blockedReason: null,
    status: "active",
  })
}

export async function reportBlocker(sessionId: string, reason: string) {
  await SessionStorage.update(sessionId, {
    status: "blocked",
    blockedReason: reason,
  })
}

export async function reportCompleted(sessionId: string) {
  await SessionStorage.update(sessionId, {
    initiative: null,
    blockedReason: null,
    status: "active",
  })
}

export async function main(argv: string[]) {
  const app = subcommands({
    name: "goddard",
    cmds: {
      "declare-initiative": command({
        name: "declare-initiative",
        description: "Declare the next initiative you are working on.",
        args: {
          title: option({
            type: string,
            long: "title",
            description: "The title of the initiative.",
          }),
        },
        handler: async (args) => {
          await declareInitiative(await requireSessionId(), args.title)
          console.log(`Initiative declared: ${args.title}`)
        },
      }),

      "report-blocker": command({
        name: "report-blocker",
        description: "Report a blocker that prevents further progress.",
        args: {
          reason: option({
            type: string,
            long: "reason",
            description: "The reason for the blocker.",
          }),
        },
        handler: async (args) => {
          await reportBlocker(await requireSessionId(), args.reason)
          console.log(`Blocker reported: ${args.reason}`)
        },
      }),

      "report-completed": command({
        name: "report-completed",
        description: "Report that the current initiative or task is completed.",
        args: {},
        handler: async () => {
          await reportCompleted(await requireSessionId())
          console.log("Work reported as completed.")
        },
      }),
    },
  })

  await run(app, argv)
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
