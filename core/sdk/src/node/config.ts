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
import { access, readFile } from "node:fs/promises"
import { z } from "zod"

/** Paths and merged root config for a single node config resolution request. */
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: GoddardRootConfigDocument
}

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

  let parsed: unknown

  try {
    parsed = JSON.parse(await readFile(path, "utf-8"))
  } catch (error) {
    throw new Error(`${label} at ${path} must be valid JSON.`, { cause: error })
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
  return readJsonConfig(path, ActionConfig, "Action config")
}

/** Reads and validates a packaged loop config document. */
export async function readLoopConfig(path: string): Promise<GoddardLoopConfigDocument | undefined> {
  return readJsonConfig(path, LoopConfig, "Loop config")
}
