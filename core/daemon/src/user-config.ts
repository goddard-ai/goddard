import { randomUUID } from "node:crypto"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { basename, dirname, join } from "node:path"
import { IpcClientError } from "@goddard-ai/ipc"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import {
  UserConfigIpcErrors,
  type GetUserConfigResponse,
  type UpdateUserConfigRequest,
  type UpdateUserConfigResponse,
  type UserConfigDocument,
} from "@goddard-ai/schema/daemon-ipc"

import { buildRootConfigSchema } from "./config-schema.ts"
import { buildEditableRootConfigJsonSchema } from "./json-schemas.ts"

const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"

/** Daemon-owned access to the persisted user configuration document. */
export type UserConfigService = {
  get: () => Promise<GetUserConfigResponse>
  update: (input: UpdateUserConfigRequest) => Promise<UpdateUserConfigResponse>
}

/** Creates serialized, validated user configuration read and mutation operations. */
export function createUserConfigService(): UserConfigService {
  const schema = buildEditableRootConfigJsonSchema()
  let pendingUpdate = Promise.resolve<unknown>(undefined)

  const update = (input: UpdateUserConfigRequest) => {
    const task = pendingUpdate.then(
      () => applyUserConfigUpdate(input),
      () => applyUserConfigUpdate(input),
    )
    pendingUpdate = task.catch(() => undefined)
    return task
  }

  return {
    async get() {
      return {
        document: await readUserConfigDocument(),
        schema,
      }
    },
    update,
  }
}

async function applyUserConfigUpdate(
  input: UpdateUserConfigRequest,
): Promise<UpdateUserConfigResponse> {
  if (input.operation === "set" && !Object.hasOwn(input, "value")) {
    throwInvalidPatch(input.path)
  }

  const current = await readUserConfigDocument()
  const next = structuredClone(current)
  applyJsonPointerUpdate(next, input)

  const parsed = buildRootConfigSchema().safeParse(next)
  if (!parsed.success) {
    throw new IpcClientError<(typeof UserConfigIpcErrors)["InvalidDocument"]>({
      code: UserConfigIpcErrors.InvalidDocument.code,
      details: {
        paths: [...new Set(parsed.error.issues.map((issue) => encodeJsonPointer(issue.path)))],
      },
    })
  }

  const document = parsed.data as UserConfigDocument
  const restartRequired = JSON.stringify(current.daemon) !== JSON.stringify(document.daemon)
  if (JSON.stringify(current) !== JSON.stringify(document)) {
    await writeUserConfigDocument(document)
  }

  return {
    document,
    restartRequired,
  }
}

async function readUserConfigDocument(): Promise<UserConfigDocument> {
  let source: string
  try {
    source = await readFile(getGlobalConfigPath(), "utf8")
  } catch (error) {
    if (isFileSystemError(error, "ENOENT")) {
      return {}
    }

    throw new IpcClientError<(typeof UserConfigIpcErrors)["Unavailable"]>(
      { code: UserConfigIpcErrors.Unavailable.code },
      { cause: error },
    )
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(source)
  } catch (error) {
    throw new IpcClientError<(typeof UserConfigIpcErrors)["InvalidDocument"]>(
      {
        code: UserConfigIpcErrors.InvalidDocument.code,
        details: { paths: [] },
      },
      { cause: error },
    )
  }

  if (!isRecord(parsed)) {
    throw new IpcClientError<(typeof UserConfigIpcErrors)["InvalidDocument"]>({
      code: UserConfigIpcErrors.InvalidDocument.code,
      details: { paths: [""] },
    })
  }

  const document = { ...parsed }
  delete document.$schema
  return document
}

async function writeUserConfigDocument(document: UserConfigDocument) {
  const configPath = getGlobalConfigPath()
  const configDir = dirname(configPath)
  const temporaryPath = join(configDir, `.${basename(configPath)}.${randomUUID()}.tmp`)

  try {
    await mkdir(configDir, { recursive: true })
    await writeFile(
      temporaryPath,
      `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...document }, null, 2)}\n`,
      { encoding: "utf8", mode: 0o600 },
    )
    await rename(temporaryPath, configPath)
  } catch (error) {
    await rm(temporaryPath, { force: true }).catch(() => {})
    throw new IpcClientError<(typeof UserConfigIpcErrors)["Unavailable"]>(
      { code: UserConfigIpcErrors.Unavailable.code },
      { cause: error },
    )
  }
}

function applyJsonPointerUpdate(document: UserConfigDocument, input: UpdateUserConfigRequest) {
  const segments = input.path.slice(1).split("/").map(decodeJsonPointerSegment)
  let parent: unknown = document

  for (const segment of segments.slice(0, -1)) {
    if (Array.isArray(parent)) {
      const index = parseArrayIndex(segment, parent.length - 1, input.path)
      parent = parent[index]
      continue
    }

    if (!isRecord(parent)) {
      throwInvalidPatch(input.path)
    }

    if (!Object.hasOwn(parent, segment)) {
      if (input.operation === "remove") {
        return
      }
      defineObjectValue(parent, segment, {})
    }
    parent = parent[segment]
  }

  const segment = segments.at(-1)!
  if (Array.isArray(parent)) {
    if (input.operation === "set") {
      if (segment === "-") {
        parent.push(input.value)
        return
      }
      const index = parseArrayIndex(segment, parent.length, input.path)
      if (index === parent.length) {
        parent.push(input.value)
      } else {
        parent[index] = input.value
      }
      return
    }

    const index = parseArrayIndex(segment, parent.length - 1, input.path)
    parent.splice(index, 1)
    return
  }

  if (!isRecord(parent)) {
    throwInvalidPatch(input.path)
  }

  if (input.operation === "set") {
    defineObjectValue(parent, segment, input.value)
  } else {
    delete parent[segment]
  }
}

function parseArrayIndex(segment: string, maximum: number, path: string) {
  if (!/^\d+$/.test(segment)) {
    throwInvalidPatch(path)
  }

  const index = Number(segment)
  if (!Number.isSafeInteger(index) || index < 0 || index > maximum) {
    throwInvalidPatch(path)
  }
  return index
}

function decodeJsonPointerSegment(segment: string) {
  return segment.replaceAll("~1", "/").replaceAll("~0", "~")
}

function encodeJsonPointer(path: PropertyKey[]) {
  if (path.length === 0) {
    return ""
  }
  return `/${path
    .map(String)
    .map((segment) => segment.replaceAll("~", "~0").replaceAll("/", "~1"))
    .join("/")}`
}

function defineObjectValue(target: Record<string, unknown>, key: string, value: unknown) {
  Object.defineProperty(target, key, {
    configurable: true,
    enumerable: true,
    value,
    writable: true,
  })
}

function throwInvalidPatch(path: string): never {
  throw new IpcClientError<(typeof UserConfigIpcErrors)["InvalidPatch"]>({
    code: UserConfigIpcErrors.InvalidPatch.code,
    details: { path },
  })
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function isFileSystemError(error: unknown, code: string) {
  return isRecord(error) && error.code === code
}
