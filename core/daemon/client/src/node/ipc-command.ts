import { listIpcRouteActions, type HttpRouteTree, type IpcRouteAction } from "@goddard-ai/ipc"
import { command, option, subcommands, type Runner, type Type } from "cmd-ts"

import { daemonIpcRoutes } from "../daemon-ipc.ts"
import type { DaemonClientEnv, DaemonIpcClientFactory } from "./index.ts"

type CommandOutput = {
  writeLine?: (line: string) => void | Promise<void>
}

type RouteCommandContext = {
  client: unknown
  output: Required<CommandOutput>
}

/** Options for the generated daemon IPC command surface. */
export type DaemonIpcCommandOptions = {
  createClient?: DaemonIpcClientFactory
  env?: DaemonClientEnv
} & CommandOutput

const JsonValue: Type<string, unknown> = {
  displayName: "json",
  async from(value) {
    try {
      return JSON.parse(value) as unknown
    } catch (error) {
      throw new Error("Expected --json to contain valid JSON.", { cause: error })
    }
  },
}

/** Creates a generated cmd-ts command tree for daemon IPC routes. */
export function createDaemonIpcCommand(options: DaemonIpcCommandOptions = {}) {
  const output = {
    writeLine: options.writeLine ?? ((line: string) => console.log(line)),
  }
  const routeMetadata = new Map(
    listIpcRouteActions(daemonIpcRoutes).map((route) => [formatRouteKey(route.keyPath), route]),
  )
  const commandTree = createRouteCommandTree(daemonIpcRoutes, {
    routeMetadata,
    async getContext() {
      const { createDaemonIpcClientFromEnv } = await import("./index.ts")
      const { client } = createDaemonIpcClientFromEnv({
        env: options.env,
        createClient: options.createClient,
      })

      return {
        client,
        output,
      }
    },
  })

  return subcommands({
    name: "ipc",
    description: "Call daemon IPC routes.",
    cmds: commandTree,
  })
}

type RouteCommandTree = Record<string, Runner<any, any>>

function createRouteCommandTree(
  routes: HttpRouteTree,
  options: {
    routeMetadata: ReadonlyMap<string, IpcRouteAction>
    getContext: () => Promise<RouteCommandContext>
  },
  routeKeyPath: readonly string[] = [],
): RouteCommandTree {
  const commands: RouteCommandTree = {}

  for (const [key, node] of Object.entries(routes)) {
    const commandName = toCommandName(key)
    const keyPath = [...routeKeyPath, key]

    commands[commandName] =
      node.kind === "resource"
        ? subcommands({
            name: commandName,
            cmds: createRouteCommandTree(node.children, options, keyPath),
          })
        : createRouteActionCommand(key, keyPath, options)
  }

  return commands
}

function createRouteActionCommand(
  key: string,
  keyPath: readonly string[],
  options: {
    routeMetadata: ReadonlyMap<string, IpcRouteAction>
    getContext: () => Promise<RouteCommandContext>
  },
) {
  const metadata = options.routeMetadata.get(formatRouteKey(keyPath))
  if (!metadata) {
    throw new Error(`Missing route metadata for ${keyPath.join(".")}`)
  }

  return command({
    name: toCommandName(key),
    description: formatRouteDescription(metadata),
    args:
      metadata.requestInput === null
        ? {}
        : {
            json: option({
              long: "json",
              type: createJsonRouteInput(metadata),
              description: "JSON request payload.",
            }),
          },
    handler: async (args: { json?: unknown }) => {
      const context = await options.getContext()
      const method = selectRouteMethod(context.client, keyPath)
      const response = await method(...(metadata.requestInput === null ? [] : [args.json]))

      if (metadata.streamsNdjson) {
        await writeStream(response, context.output)
        return
      }

      await context.output.writeLine(JSON.stringify(response))
    },
  })
}

function createJsonRouteInput(route: IpcRouteAction): Type<string, unknown> {
  if (route.requestInput === null) {
    return JsonValue
  }

  const schema = route.action.schema[route.requestInput]
  if (!schema || typeof schema !== "object" || !("parse" in schema)) {
    throw new Error(`Route ${route.keyPath.join(".")} is missing a request validator.`)
  }

  return {
    ...JsonValue,
    async from(value) {
      const parsed = await JsonValue.from(value)
      return (schema.parse as (value: unknown) => unknown)(parsed)
    },
  }
}

function selectRouteMethod(client: unknown, keyPath: readonly string[]) {
  let cursor: unknown = client

  for (const key of keyPath) {
    if (!cursor || typeof cursor !== "object" || !(key in cursor)) {
      throw new Error(`Daemon IPC client is missing route ${keyPath.join(".")}`)
    }
    cursor = (cursor as Record<string, unknown>)[key]
  }

  if (typeof cursor !== "function") {
    throw new Error(`Daemon IPC client route ${keyPath.join(".")} is not callable.`)
  }

  return cursor as (...args: unknown[]) => Promise<unknown>
}

async function writeStream(stream: unknown, output: Required<CommandOutput>) {
  if (!isAsyncIterable(stream)) {
    throw new Error("Daemon IPC stream route did not return an async iterable.")
  }

  for await (const event of stream) {
    await output.writeLine(JSON.stringify(event))
  }
}

function isAsyncIterable(value: unknown): value is AsyncIterable<unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    Symbol.asyncIterator in value &&
    typeof value[Symbol.asyncIterator] === "function"
  )
}

function formatRouteDescription(route: IpcRouteAction) {
  return `${route.action.method} /${route.httpPath.join("/")}`
}

function toCommandName(value: string) {
  return value.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`)
}

function formatRouteKey(keyPath: readonly string[]) {
  return keyPath.join("\0")
}
