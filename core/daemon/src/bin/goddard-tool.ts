#!/usr/bin/env node
import * as fs from "node:fs/promises"
import { createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client/node"
import { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type { DaemonSession, SessionInboxMetadataInput } from "@goddard-ai/schema/daemon"
import { command, option, optional, run, string, subcommands } from "cmd-ts"

async function requireSessionId(): Promise<DaemonSession["id"]> {
  const { client } = createDaemonIpcClientFromEnv()
  const result = await client.send("sessionResolveToken", {
    token: requireSessionToken(),
  })
  return DaemonSessionId.parse(result.id)
}

function requireSessionToken(): string {
  return requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
}

export async function declareInitiative(sessionId: DaemonSession["id"], title: string) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("sessionDeclareInitiative", { id: sessionId, title })
}

export async function reportBlocker(
  sessionId: DaemonSession["id"],
  reason: string,
  metadata: SessionInboxMetadataInput = {},
) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("sessionReportBlocker", { id: sessionId, reason, ...metadata })
}

export async function reportTurnEnded(
  sessionId: DaemonSession["id"],
  metadata: SessionInboxMetadataInput = {},
) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("sessionReportTurnEnded", { id: sessionId, ...metadata })
}

export async function submitPr(
  title: string,
  body: string,
  metadata: SessionInboxMetadataInput = {},
) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("prSubmit", {
    token: requireSessionToken(),
    cwd: process.cwd(),
    title,
    body,
    ...metadata,
  })
}

export async function replyPr(message: string, metadata: SessionInboxMetadataInput = {}) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.send("prReply", {
    token: requireSessionToken(),
    cwd: process.cwd(),
    message,
    ...metadata,
  })
}

function metadataOptions() {
  return {
    scope: option({
      type: optional(string),
      long: "scope",
      description: "Short inbox scope for this turn.",
    }),
    headline: option({
      type: optional(string),
      long: "headline",
      description: "Short inbox headline for this turn.",
    }),
    metadataJson: option({
      type: optional(string),
      long: "json",
      description: "JSON inbox metadata object with optional scope and headline.",
    }),
  }
}

function resolveMetadataInput(args: {
  scope?: string
  headline?: string
  metadataJson?: string
}): SessionInboxMetadataInput {
  let parsed: SessionInboxMetadataInput = {}
  if (args.metadataJson) {
    const value = JSON.parse(args.metadataJson) as unknown
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      throw new Error("--json must be an object")
    }

    const record = value as Record<string, unknown>
    parsed = {
      scope: typeof record.scope === "string" ? record.scope : undefined,
      headline: typeof record.headline === "string" ? record.headline : undefined,
    }
  }

  return {
    scope: args.scope ?? parsed.scope,
    headline: args.headline ?? parsed.headline,
  }
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
          ...metadataOptions(),
        },
        handler: async (args) => {
          const reason = await fs.readFile(args.reasonFile, "utf-8")
          await reportBlocker(await requireSessionId(), reason, resolveMetadataInput(args))
          console.log(`Blocker reported from file: ${args.reasonFile}`)
        },
      }),

      "end-turn": command({
        name: "end-turn",
        description: "Report that the current turn has ended.",
        args: {
          ...metadataOptions(),
        },
        handler: async (args) => {
          await reportTurnEnded(await requireSessionId(), resolveMetadataInput(args))
          console.log("Turn ended.")
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
          ...metadataOptions(),
        },
        handler: async (args) => {
          const body = await fs.readFile(args.bodyFile, "utf-8")
          await submitPr(args.title, body, resolveMetadataInput(args))
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
          ...metadataOptions(),
        },
        handler: async (args) => {
          const message = await fs.readFile(args.messageFile, "utf-8")
          await replyPr(message, resolveMetadataInput(args))
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
