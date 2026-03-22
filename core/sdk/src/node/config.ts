import { mergeRootConfigLayers } from "@goddard-ai/config"
import { ActionConfig, LoopConfig, UserConfig } from "@goddard-ai/schema/config"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/storage"
import { constants as fsConstants } from "node:fs"
import { access, readFile, writeFile } from "node:fs/promises"
import { z } from "zod"

/** Paths and merged root config for a single node config resolution request. */
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: UserConfig
}

const SCHEMA_BASE_URL =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/"

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
  schemaReference: string,
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
    try {
      parsed.$schema = new URL(schemaReference, SCHEMA_BASE_URL).toString()
      await writeFile(path, JSON.stringify(parsed, null, 2))
    } catch (e) {
      // Ignore errors if the schema file cannot be resolved or written
    }
  }

  let normalized = parsed
  if (typeof parsed === "object" && parsed !== null && "$schema" in parsed) {
    normalized = { ...parsed }
    delete (normalized as any).$schema
  }

  const result = schema.safeParse(normalized)
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
      await readJsonConfig(getGlobalConfigPath(), UserConfig, "Global config", "goddard.json"),
      await readJsonConfig(getLocalConfigPath(cwd), UserConfig, "Local config", "goddard.json"),
    ),
  }
}

/** Reads and validates a packaged action config document. */
export async function readActionConfig(path: string): Promise<ActionConfig | undefined> {
  return readJsonConfig(path, ActionConfig, "Action config", "action.json")
}

/** Reads and validates a packaged loop config document. */
export async function readLoopConfig(path: string): Promise<LoopConfig | undefined> {
  return readJsonConfig(path, LoopConfig, "Loop config", "loop.json")
}
