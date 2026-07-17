import { realpath } from "node:fs/promises"
import { resolve } from "node:path"

import { TaskErrorCodes } from "../schema.ts"
import { createTaskIpcError } from "./ipc-error.ts"

/** Normalizes the repository root used as durable task scope. */
export async function normalizeTaskRootDir(rootDir: string) {
  try {
    return await realpath(resolve(rootDir))
  } catch {
    throw createTaskIpcError(TaskErrorCodes.InvalidRoot, { rootDir })
  }
}
