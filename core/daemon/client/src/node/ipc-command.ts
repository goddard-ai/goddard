import { listIpcRouteActions, type HttpRouteTree, type IpcRouteAction } from "@goddard-ai/ipc"
import { command, option, optional, subcommands, type Runner, type Type } from "cmd-ts"
import { toJSONSchema } from "zod"
import type { ToJSONSchemaParams } from "zod/v4/core"

import { daemonIpcRoutes } from "../daemon-ipc.ts"
import type { DaemonClientEnv, DaemonIpcClientFactory } from "./index.ts"

type CommandOutput = {
  writeLine?: (line: string) => void | Promise<void>
}

type RouteCommandContext = {
  client: unknown
  output: Required<CommandOutput>
}

type JsonSchema = boolean | { [key: string]: unknown }

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
    output,
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
    output: Required<CommandOutput>
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
    output: Required<CommandOutput>
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
              type: optional(createJsonRouteInput(metadata)),
              description: "JSON request payload.",
            }),
          },
    handler: async (args: { json?: unknown }) => {
      if (metadata.requestInput !== null && args.json === undefined) {
        await options.output.writeLine(JSON.stringify(createExpectedJsonShape(metadata), null, 2))
        return
      }

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

function createExpectedJsonShape(route: IpcRouteAction) {
  if (route.requestInput === null) {
    return {}
  }

  const schema = route.action.schema[route.requestInput]
  if (!schema || typeof schema !== "object") {
    return {}
  }

  const jsonSchema = toJSONSchema(schema as never, jsonSchemaParams)
  return createShapeFromJsonSchema(jsonSchema, jsonSchema)
}

const jsonSchemaParams: ToJSONSchemaParams = {
  io: "input",
  unrepresentable: "any",
}

function createShapeFromJsonSchema(schema: JsonSchema, rootSchema: JsonSchema): unknown {
  if (schema === true || schema === false) {
    return "<value>"
  }

  const resolvedSchema = resolveJsonSchemaRef(schema, rootSchema)
  if (resolvedSchema !== schema) {
    return createShapeFromJsonSchema(resolvedSchema, rootSchema)
  }

  if (Array.isArray(resolvedSchema.enum) && resolvedSchema.enum.length > 0) {
    return resolvedSchema.enum[0]
  }
  if ("const" in resolvedSchema) {
    return resolvedSchema.const
  }

  const unionSchema = readFirstSchema(resolvedSchema.anyOf ?? resolvedSchema.oneOf)
  if (unionSchema) {
    return createShapeFromJsonSchema(unionSchema, rootSchema)
  }

  const type = resolveJsonSchemaType(resolvedSchema.type)
  if (type === "object") {
    return createObjectShape(resolvedSchema, rootSchema)
  }
  if (type === "array") {
    return [createShapeFromJsonSchema(readFirstSchema(resolvedSchema.items) ?? true, rootSchema)]
  }
  if (type === "string") {
    return "<string>"
  }
  if (type === "number" || type === "integer") {
    return 0
  }
  if (type === "boolean") {
    return false
  }
  if (type === "null") {
    return null
  }

  return "<value>"
}

function createObjectShape(schema: Record<string, unknown>, rootSchema: JsonSchema) {
  const properties =
    schema.properties && typeof schema.properties === "object"
      ? (schema.properties as Record<string, JsonSchema>)
      : {}
  const shape: Record<string, unknown> = {}

  for (const [key, propertySchema] of Object.entries(properties)) {
    shape[key] = createShapeFromJsonSchema(propertySchema, rootSchema)
  }

  return shape
}

function resolveJsonSchemaType(type: unknown) {
  if (typeof type === "string") {
    return type
  }
  if (Array.isArray(type)) {
    return type.find((value) => typeof value === "string" && value !== "null") ?? type[0]
  }
  return undefined
}

function readFirstSchema(value: unknown): JsonSchema | undefined {
  if (Array.isArray(value)) {
    return value.find(isJsonSchema)
  }
  return isJsonSchema(value) ? value : undefined
}

function resolveJsonSchemaRef(schema: Record<string, unknown>, rootSchema: JsonSchema): JsonSchema {
  if (typeof schema.$ref !== "string" || !schema.$ref.startsWith("#/$defs/")) {
    return schema
  }
  if (rootSchema === false || rootSchema === true || !rootSchema.$defs) {
    return schema
  }

  const key = schema.$ref.slice("#/$defs/".length)
  const defs =
    typeof rootSchema.$defs === "object" && rootSchema.$defs !== null
      ? (rootSchema.$defs as Record<string, unknown>)
      : {}
  const definition = defs[key]
  return isJsonSchema(definition) ? definition : schema
}

function isJsonSchema(value: unknown): value is JsonSchema {
  return typeof value === "boolean" || (typeof value === "object" && value !== null)
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
