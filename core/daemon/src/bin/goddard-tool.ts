#!/usr/bin/env node
import * as fs from "node:fs/promises"
import { basename, join } from "node:path"
import { createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client/node"
import { getGoddardTempLogDir } from "@goddard-ai/paths/node"
import type { AttentionMetadataInput } from "@goddard-ai/schema/attention"
import { SessionId, type DaemonSession } from "@goddard-ai/session/schema"
import { command, oneOf, option, optional, positional, run, string, subcommands } from "cmd-ts"

const logSurfaces = ["app", "daemon", "agent-process"] as const
const defaultLogLineLimit = 200

type LogSurface = (typeof logSurfaces)[number]

async function requireSessionId(): Promise<DaemonSession["id"]> {
  const { client } = createDaemonIpcClientFromEnv()
  const result = await client.session.resolveToken({
    token: requireSessionToken(),
  })
  return SessionId.parse(result.id)
}

function requireSessionToken(): string {
  return requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")
}

export async function declareInitiative(sessionId: DaemonSession["id"], title: string) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.session.declareInitiative({ id: sessionId, title })
}

export async function reportBlocker(
  sessionId: DaemonSession["id"],
  reason: string,
  metadata: AttentionMetadataInput = {},
) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.session.reportBlocker({ id: sessionId, reason, ...metadata })
}

export async function reportTurnEnded(
  sessionId: DaemonSession["id"],
  metadata: AttentionMetadataInput = {},
) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.session.reportTurnEnded({ id: sessionId, ...metadata })
}

export async function submitPr(title: string, body: string, metadata: AttentionMetadataInput = {}) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.pr.submit({
    token: requireSessionToken(),
    cwd: process.cwd(),
    title,
    body,
    ...metadata,
  })
}

export async function replyPr(message: string, metadata: AttentionMetadataInput = {}) {
  const { client } = createDaemonIpcClientFromEnv()
  await client.pr.reply({
    token: requireSessionToken(),
    cwd: process.cwd(),
    message,
    ...metadata,
  })
}

export async function readLogSurface(input: {
  surface: LogSurface
  lines?: string
  logDir?: string
}) {
  const logDir = input.logDir ?? getGoddardTempLogDir()
  const paths = await resolveLogSurfacePaths(input.surface, logDir)
  if (paths.length === 0) {
    return `No ${input.surface} logs found in ${logDir}`
  }

  const lineLimit = resolveLineLimit(input.lines)
  const content = await Promise.all(
    paths.map(async (path) => {
      const text = await fs.readFile(path, "utf-8")
      const body = takeLastLines(text, lineLimit)
      return paths.length === 1 ? body : [`== ${basename(path)} ==`, body].join("\n")
    }),
  )

  return content.join("\n")
}

async function resolveLogSurfacePaths(surface: LogSurface, logDir: string) {
  if (surface === "app") {
    return await existingPaths([join(logDir, "app.log")])
  }

  if (surface === "daemon") {
    return await existingPaths([join(logDir, "daemon.log")])
  }

  const entries = await fs.readdir(logDir, { withFileTypes: true }).catch(() => [])
  return entries
    .filter(
      (entry) =>
        entry.isFile() &&
        entry.name.startsWith("agent-process-") &&
        entry.name.endsWith(".stderr.log"),
    )
    .map((entry) => join(logDir, entry.name))
    .sort()
}

async function existingPaths(paths: string[]) {
  const existing = await Promise.all(
    paths.map(async (path) => ((await fs.stat(path).catch(() => null))?.isFile() ? path : null)),
  )
  return existing.filter((path): path is string => path !== null)
}

function resolveLineLimit(value: string | undefined) {
  if (!value) {
    return defaultLogLineLimit
  }

  const lineLimit = Number(value)
  if (!Number.isInteger(lineLimit) || lineLimit < 0) {
    throw new Error("--lines must be a non-negative integer")
  }

  return lineLimit
}

function takeLastLines(content: string, lineLimit: number) {
  if (lineLimit === 0) {
    return content.trimEnd()
  }

  return content.trimEnd().split("\n").slice(-lineLimit).join("\n")
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
}): AttentionMetadataInput {
  let parsed: AttentionMetadataInput = {}
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

      logs: command({
        name: "logs",
        description: "Print recent Goddard process logs for one surface.",
        args: {
          surface: positional({
            type: oneOf(logSurfaces),
            displayName: "surface",
            description: "Log surface to inspect: app, daemon, or agent-process.",
          }),
          lines: option({
            type: optional(string),
            long: "lines",
            description: "Number of trailing lines to print. Use 0 for the full log.",
          }),
        },
        handler: async (args) => {
          console.log(await readLogSurface(args))
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
