import { mergeRootConfigLayers } from "@goddard-ai/config"
import {
  ActionConfig,
  LoopConfig,
  RootConfig,
  type GoddardActionConfigDocument,
  type GoddardLoopConfigDocument,
  type GoddardRootConfigDocument,
} from "@goddard-ai/schema/config"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/storage"
import { constants as fsConstants } from "node:fs"
import { access, readFile, writeFile } from "node:fs/promises"
import { dirname, relative } from "node:path"
import { createRequire } from "node:module"
import { z } from "zod"

/** Paths and merged root config for a single node config resolution request. */
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: GoddardRootConfigDocument
}

const require = createRequire(import.meta.url)

/** Returns true when a filesystem path exists. */
export async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Reads and validates a JSON config document when the file exists. */
export async function readJsonConfig<T>(
  path: string,
  schema: z.ZodType<T>,
  label: string,
): Promise<T | undefined> {
  if (!(await pathExists(path))) {
    return undefined
  }

  let parsed: any

  try {
    parsed = JSON.parse(await readFile(path, "utf-8"))
  } catch (error) {
    throw new Error(`${label} at ${path} must be valid JSON.`, { cause: error })
  }

  if (typeof parsed === "object" && parsed !== null && !("$schema" in parsed)) {
    let schemaFileName = "goddard.json"
    if (label === "Action config") {
      schemaFileName = "action.json"
    } else if (label === "Loop config") {
      schemaFileName = "loop.json"
    }

    try {
      // Use require.resolve dynamically to find the schema file path from this module
      const resolvedSchemaPath = require.resolve(`@goddard-ai/schema/json/${schemaFileName}`)
      const relPath = relative(dirname(path), resolvedSchemaPath)
      parsed.$schema = relPath
      await writeFile(path, JSON.stringify(parsed, null, 2))
    } catch (e) {
      // Ignore errors if the schema file cannot be resolved or written
    }
  }

  const result = schema.safeParse(parsed)
  if (!result.success) {
    throw new Error(`${label} at ${path} is invalid: ${z.prettifyError(result.error)}`)
  }

  return result.data
}

/** Reads and merges the global and local root config documents for the given cwd. */
export async function readMergedRootConfig(
  cwd: string = process.cwd(),
): Promise<ResolvedConfigRoots> {
  const globalRoot = getGoddardGlobalDir()
  const localRoot = getGoddardLocalDir(cwd)

  return {
    globalRoot,
    localRoot,
    config: mergeRootConfigLayers(
      await readJsonConfig(getGlobalConfigPath(), RootConfig, "Global config"),
      await readJsonConfig(getLocalConfigPath(cwd), RootConfig, "Local config"),
    ),
  }
}

/** Reads and validates a packaged action config document. */
export async function readActionConfig(
  path: string,
): Promise<GoddardActionConfigDocument | undefined> {
  const result = await readJsonConfig(path, ActionConfig, "Action config")
  if (result && "$schema" in result) {
    delete (result as any).$schema
  }
  return result
}

/** Reads and validates a packaged loop config document. */
export async function readLoopConfig(path: string): Promise<GoddardLoopConfigDocument | undefined> {
  const result = await readJsonConfig(path, LoopConfig, "Loop config")
  if (result && "$schema" in result) {
    delete (result as any).$schema
  }
  return result
}
