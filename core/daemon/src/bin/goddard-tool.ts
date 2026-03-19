#!/usr/bin/env node
import { createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client"
import { SessionStorage } from "@goddard-ai/storage"
import { command, option, run, string, subcommands } from "cmd-ts"
import * as fs from "node:fs/promises"

async function requireSessionId(): Promise<string> {
  const { client } = createDaemonIpcClientFromEnv()
  const sessionToken = requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
  const result = await client.send("sessionResolveToken", { token: sessionToken })
  return result.id
}

export async function declareInitiative(sessionId: string, title: string) {
  await SessionStorage.update(sessionId, {
    status: "active",
    initiative: title,
    blockedReason: null,
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
    status: "done",
    initiative: null,
    blockedReason: null,
  })
}

export async function submitPr(sessionId: string, title: string, body: string) {
  const { client } = createDaemonIpcClientFromEnv()
  const sessionToken = requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
  const pr = await client.send("prSubmit", {
    token: sessionToken,
    cwd: process.cwd(),
    title,
    body,
  })

  await SessionStorage.update(sessionId, {
    status: "done",
    lastAgentMessage: `PR Submitted: ${title}\n${pr.url}\n\n${body}`,
  })
}

export async function replyPr(sessionId: string, message: string) {
  const { client } = createDaemonIpcClientFromEnv()
  const sessionToken = requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
  await client.send("prReply", {
    token: sessionToken,
    cwd: process.cwd(),
    message,
  })

  await SessionStorage.update(sessionId, {
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
