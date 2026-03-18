#!/usr/bin/env node
import { createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client"
import { command, option, optional, run, string, subcommands } from "cmd-ts"
import * as fs from "node:fs/promises"

async function readOptionalFile(path: string | undefined): Promise<string | undefined> {
  if (!path) {
    return undefined
  }

  return fs.readFile(path, "utf-8")
}

function requiredEnv(value: string | undefined, name: string): string {
  if (!value) {
    throw new Error(`${name} is required`)
  }

  return value
}

function requireWorkforceRootDir(): string {
  return requiredEnv(process.env.GODDARD_WORKFORCE_ROOT_DIR, "GODDARD_WORKFORCE_ROOT_DIR")
}

function requireWorkforceRequestId(value?: string): string {
  return (
    value ?? requiredEnv(process.env.GODDARD_WORKFORCE_REQUEST_ID, "GODDARD_WORKFORCE_REQUEST_ID")
  )
}

function optionalSessionToken(): string | undefined {
  return process.env.GODDARD_SESSION_TOKEN
}

export async function workforceRequest(targetAgentId: string, input: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceRequest", {
    rootDir: requireWorkforceRootDir(),
    targetAgentId,
    input,
    token: optionalSessionToken(),
  })
}

export async function workforceUpdate(requestId: string, input: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceUpdate", {
    rootDir: requireWorkforceRootDir(),
    requestId,
    input,
    token: optionalSessionToken(),
  })
}

export async function workforceCancel(requestId: string, reason?: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceCancel", {
    rootDir: requireWorkforceRootDir(),
    requestId,
    reason,
    token: optionalSessionToken(),
  })
}

export async function workforceTruncate(agentId?: string, reason?: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceTruncate", {
    rootDir: requireWorkforceRootDir(),
    agentId,
    reason,
    token: optionalSessionToken(),
  })
}

export async function workforceRespond(requestId: string, output: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceRespond", {
    rootDir: requireWorkforceRootDir(),
    requestId,
    output,
    token: requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN"),
  })
}

export async function workforceSuspend(requestId: string, reason: string) {
  const { client } = createDaemonIpcClientFromEnv()
  return client.send("workforceSuspend", {
    rootDir: requireWorkforceRootDir(),
    requestId,
    reason,
    token: requiredEnv(process.env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN"),
  })
}

export async function main(argv: string[]) {
  const app = subcommands({
    name: "workforce",
    cmds: {
      request: command({
        name: "request",
        description: "Append a delegated workforce request.",
        args: {
          targetAgentId: option({
            type: string,
            long: "target-agent-id",
            description: "The receiving workforce agent id.",
          }),
          inputFile: option({
            type: string,
            long: "input-file",
            description: "The file containing the request payload.",
          }),
        },
        handler: async (args) => {
          const response = await workforceRequest(
            args.targetAgentId,
            await fs.readFile(args.inputFile, "utf-8"),
          )
          console.log(`Workforce request appended: ${response.requestId ?? "unknown"}`)
        },
      }),
      update: command({
        name: "update",
        description: "Append an update to an existing workforce request.",
        args: {
          requestId: option({
            type: string,
            long: "request-id",
            description: "The target workforce request id.",
          }),
          inputFile: option({
            type: string,
            long: "input-file",
            description: "The file containing the update payload.",
          }),
        },
        handler: async (args) => {
          await workforceUpdate(args.requestId, await fs.readFile(args.inputFile, "utf-8"))
          console.log(`Workforce request updated: ${args.requestId}`)
        },
      }),
      cancel: command({
        name: "cancel",
        description: "Cancel an existing workforce request.",
        args: {
          requestId: option({
            type: string,
            long: "request-id",
            description: "The target workforce request id.",
          }),
          reasonFile: option({
            type: optional(string),
            long: "reason-file",
            description: "The file containing the optional cancel reason.",
          }),
        },
        handler: async (args) => {
          await workforceCancel(args.requestId, await readOptionalFile(args.reasonFile))
          console.log(`Workforce request cancelled: ${args.requestId}`)
        },
      }),
      truncate: command({
        name: "truncate",
        description: "Truncate pending work for one workforce scope.",
        args: {
          agentId: option({
            type: string,
            long: "agent-id",
            description: "The optional workforce agent id to truncate.",
            defaultValue: () => "",
          }),
          reasonFile: option({
            type: optional(string),
            long: "reason-file",
            description: "The file containing the optional truncate reason.",
          }),
        },
        handler: async (args) => {
          await workforceTruncate(
            args.agentId || undefined,
            await readOptionalFile(args.reasonFile),
          )
          console.log(`Workforce queue truncated${args.agentId ? ` for ${args.agentId}` : ""}.`)
        },
      }),
      respond: command({
        name: "respond",
        description: "Respond to the current workforce request.",
        args: {
          requestId: option({
            type: string,
            long: "request-id",
            description: "The target workforce request id.",
            defaultValue: () => process.env.GODDARD_WORKFORCE_REQUEST_ID ?? "",
          }),
          outputFile: option({
            type: string,
            long: "output-file",
            description: "The file containing the response payload.",
          }),
        },
        handler: async (args) => {
          const requestId = requireWorkforceRequestId(args.requestId || undefined)
          await workforceRespond(requestId, await fs.readFile(args.outputFile, "utf-8"))
          console.log(`Workforce request responded: ${requestId}`)
        },
      }),
      suspend: command({
        name: "suspend",
        description: "Suspend the current workforce request.",
        args: {
          requestId: option({
            type: string,
            long: "request-id",
            description: "The target workforce request id.",
            defaultValue: () => process.env.GODDARD_WORKFORCE_REQUEST_ID ?? "",
          }),
          reasonFile: option({
            type: string,
            long: "reason-file",
            description: "The file containing the suspend reason.",
          }),
        },
        handler: async (args) => {
          const requestId = requireWorkforceRequestId(args.requestId || undefined)
          await workforceSuspend(requestId, await fs.readFile(args.reasonFile, "utf-8"))
          console.log(`Workforce request suspended: ${requestId}`)
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
