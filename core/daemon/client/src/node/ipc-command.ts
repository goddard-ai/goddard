import { listIpcRouteActions, type HttpRouteTree, type IpcRouteAction } from "@goddard-ai/ipc"
import {
  number as cmdNumber,
  string as cmdString,
  command,
  option,
  optional,
  subcommands,
  type Runner,
  type Type,
} from "cmd-ts"
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
type ScalarRouteField = {
  key: string
  long: string
  type: "boolean" | "number" | "string"
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
            description: node.metadata?.description,
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
    args: createRouteArgs(metadata) as any,
    handler: async (args: Record<string, unknown>) => {
      const payload = resolveRoutePayload(metadata, args)
      if (metadata.requestInput !== null && payload === undefined) {
        await options.output.writeLine(JSON.stringify(createExpectedJsonShape(metadata), null, 2))
        return
      }

      const context = await options.getContext()
      const method = selectRouteMethod(context.client, keyPath)
      const response = await method(...(metadata.requestInput === null ? [] : [payload]))

      if (metadata.streamsNdjson) {
        await writeStream(response, context.output)
        return
      }

      await context.output.writeLine(JSON.stringify(response))
    },
  })
}

function createRouteArgs(route: IpcRouteAction) {
  if (route.requestInput === null) {
    return {}
  }

  const args: Record<string, unknown> = {
    json: option({
      long: "json",
      type: optional(createJsonRouteInput(route)),
      description: "JSON request payload.",
    }),
  }

  for (const field of listScalarRouteFields(route)) {
    args[field.key] = createScalarRouteOption(field)
  }

  return args
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

function createScalarRouteOption(field: ScalarRouteField) {
  if (field.type === "number") {
    return option({
      long: field.long,
      type: optional(cmdNumber),
      description: `JSON number field ${field.key}.`,
    })
  }

  if (field.type === "boolean") {
    return option({
      long: field.long,
      type: optional(BooleanString),
      description: `JSON boolean field ${field.key}.`,
    })
  }

  return option({
    long: field.long,
    type: optional(cmdString),
    description: `JSON string field ${field.key}.`,
  })
}

const BooleanString: Type<string, boolean> = {
  displayName: "true/false",
  async from(value) {
    if (value === "true") {
      return true
    }
    if (value === "false") {
      return false
    }
    throw new Error(`Expected boolean value to be either "true" or "false".`)
  },
}

function resolveRoutePayload(route: IpcRouteAction, args: Record<string, unknown>) {
  if (route.requestInput === null) {
    return undefined
  }

  if (args.json !== undefined) {
    return args.json
  }

  const payload = createScalarPayload(route, args)
  return payload === undefined ? undefined : validateRoutePayload(route, payload)
}

function createScalarPayload(route: IpcRouteAction, args: Record<string, unknown>) {
  const fields = listScalarRouteFields(route)
  const payload: Record<string, unknown> = {}

  for (const field of fields) {
    const value = args[field.key]
    if (value !== undefined) {
      payload[field.key] = value
    }
  }

  return Object.keys(payload).length === 0 ? undefined : payload
}

function validateRoutePayload(route: IpcRouteAction, payload: unknown) {
  if (route.requestInput === null) {
    return undefined
  }

  const schema = route.action.schema[route.requestInput]
  if (!schema || typeof schema !== "object" || !("parse" in schema)) {
    throw new Error(`Route ${route.keyPath.join(".")} is missing a request validator.`)
  }

  return (schema.parse as (value: unknown) => unknown)(payload)
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

function listScalarRouteFields(route: IpcRouteAction): ScalarRouteField[] {
  if (route.requestInput === null) {
    return []
  }

  const schema = route.action.schema[route.requestInput]
  if (!schema || typeof schema !== "object") {
    return []
  }

  return listScalarFieldsFromJsonSchema(toJSONSchema(schema as never, jsonSchemaParams))
}

function listScalarFieldsFromJsonSchema(schema: JsonSchema): ScalarRouteField[] {
  if (schema === true || schema === false || schema.type !== "object") {
    return []
  }

  const properties =
    schema.properties && typeof schema.properties === "object"
      ? (schema.properties as Record<string, JsonSchema>)
      : {}

  return Object.entries(properties).flatMap(([key, propertySchema]) => {
    if (key === "json" || propertySchema === true || propertySchema === false) {
      return []
    }

    const type = resolveJsonSchemaType(propertySchema.type)
    if (type === "string" || type === "boolean") {
      return [{ key, long: toCommandName(key), type }]
    }
    if (type === "number" || type === "integer") {
      return [{ key, long: toCommandName(key), type: "number" }]
    }

    return []
  })
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
  return route.action.metadata?.description ?? `${route.action.method} /${route.httpPath.join("/")}`
}

function toCommandName(value: string) {
  return value.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`)
}

function formatRouteKey(keyPath: readonly string[]) {
  return keyPath.join("\0")
}
