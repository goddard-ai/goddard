#!/usr/bin/env node
import { createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client/node"
import { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import { command, option, run, string, subcommands } from "cmd-ts"
import * as fs from "node:fs/promises"
import { db } from "../persistence/store.ts"

async function requireSessionId(): Promise<DaemonSession["id"]> {
  const { client } = createDaemonIpcClientFromEnv()
  const result = await client.send("sessionResolveToken", { token: requireSessionToken() })
  return DaemonSessionId.parse(result.id)
}

function requireSessionToken(): string {
  return requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
}

export async function declareInitiative(sessionId: DaemonSession["id"], title: string) {
  const record = db.sessions.get(sessionId)
  if (!record) {
    throw new Error(`Unknown session: ${sessionId}`)
  }

  db.sessions.update(sessionId, {
    status: "active",
    initiative: title,
    blockedReason: null,
  })
}

export async function reportBlocker(sessionId: DaemonSession["id"], reason: string) {
  const record = db.sessions.get(sessionId)
  if (!record) {
    throw new Error(`Unknown session: ${sessionId}`)
  }

  db.sessions.update(sessionId, {
    status: "blocked",
    blockedReason: reason,
  })
}

export async function reportCompleted(sessionId: DaemonSession["id"]) {
  const record = db.sessions.get(sessionId)
  if (!record) {
    throw new Error(`Unknown session: ${sessionId}`)
  }

  db.sessions.update(sessionId, {
    status: "done",
    initiative: null,
    blockedReason: null,
  })
}

export async function submitPr(sessionId: DaemonSession["id"], title: string, body: string) {
  const { client } = createDaemonIpcClientFromEnv()
  const pr = await client.send("prSubmit", {
    token: requireSessionToken(),
    cwd: process.cwd(),
    title,
    body,
  })

  const record = db.sessions.get(sessionId)
  if (!record) {
    throw new Error(`Unknown session: ${sessionId}`)
  }

  db.sessions.update(sessionId, {
    status: "done",
    lastAgentMessage: `PR Submitted: ${title}\n${pr.url}\n\n${body}`,
  })
}

export async function replyPr(sessionId: DaemonSession["id"], message: string) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("prReply", {
    token: requireSessionToken(),
    cwd: process.cwd(),
    message,
  })

  const record = db.sessions.get(sessionId)
  if (!record) {
    throw new Error(`Unknown session: ${sessionId}`)
  }

  db.sessions.update(sessionId, {
    status: "done",
    lastAgentMessage: `PR Reply: ${message}`,
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
          reasonFile: option({
            type: string,
            long: "reason-file",
            description: "The file containing the reason for the blocker.",
          }),
        },
        handler: async (args) => {
          const reason = await fs.readFile(args.reasonFile, "utf-8")
          await reportBlocker(await requireSessionId(), reason)
          console.log(`Blocker reported from file: ${args.reasonFile}`)
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

      "submit-pr": command({
        name: "submit-pr",
        description: "Submit a pull request.",
        args: {
          title: option({
            type: string,
            long: "title",
            description: "The title of the PR.",
          }),
          bodyFile: option({
            type: string,
            long: "body-file",
            description: "The file containing the body of the PR.",
          }),
        },
        handler: async (args) => {
          const body = await fs.readFile(args.bodyFile, "utf-8")
          await submitPr(await requireSessionId(), args.title, body)
          console.log(`PR submitted with title: ${args.title}`)
        },
      }),

      "reply-pr": command({
        name: "reply-pr",
        description: "Reply to a pull request feedback.",
        args: {
          messageFile: option({
            type: string,
            long: "message-file",
            description: "The file containing the reply message.",
          }),
        },
        handler: async (args) => {
          const message = await fs.readFile(args.messageFile, "utf-8")
          await replyPr(await requireSessionId(), message)
          console.log(`PR replied from file: ${args.messageFile}`)
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

function requiredEnv(value: string | undefined, name: string): string {
  if (!value) {
    throw new Error(`${name} is required`)
  }

  return value
}
