import {
  actionConfigSchema,
  loopConfigSchema,
  mergeRootConfigLayers,
  rootConfigSchema,
  type GoddardActionConfigDocument,
  type GoddardLoopConfigDocument,
  type GoddardRootConfigDocument,
} from "@goddard-ai/config"
import { getGlobalConfigPath, getGoddardGlobalDir, getLocalConfigPath } from "@goddard-ai/storage"
import { constants as fsConstants } from "node:fs"
import { access, readFile } from "node:fs/promises"
import { join } from "node:path"
import { z } from "zod"

// Paths and merged root config for a single resolution request.
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: GoddardRootConfigDocument
}

export async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

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

export async function readMergedRootConfig(
  cwd: string = process.cwd(),
): Promise<ResolvedConfigRoots> {
  const globalRoot = getGoddardGlobalDir()
  const localRoot = join(cwd, ".goddard")

  const globalConfig = await readJsonConfig(
    getGlobalConfigPath(),
    rootConfigSchema,
    "Global config",
  )
  const localConfig = await readJsonConfig(
    getLocalConfigPath(cwd),
    rootConfigSchema,
    "Local config",
  )

  return {
    globalRoot,
    localRoot,
    config: mergeRootConfigLayers(globalConfig, localConfig),
  }
}

export async function readActionConfig(
  path: string,
): Promise<GoddardActionConfigDocument | undefined> {
  return readJsonConfig(path, actionConfigSchema, "Action config")
}

export async function readLoopConfig(path: string): Promise<GoddardLoopConfigDocument | undefined> {
  return readJsonConfig(path, loopConfigSchema, "Loop config")
}
