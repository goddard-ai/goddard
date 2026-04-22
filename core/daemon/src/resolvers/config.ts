import { constants as fsConstants } from "node:fs"
import { access, readFile, writeFile } from "node:fs/promises"
import { mergeRootConfigLayers } from "@goddard-ai/config"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import { ActionConfig, LoopConfig, UserConfig } from "@goddard-ai/schema/config"
import { z } from "zod"

/** Paths and merged root config for one daemon-side config resolution request. */
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: UserConfig
}

/** Minimal root-config provider contract shared by daemon resolvers and the config manager. */
export type RootConfigProvider = {
  getRootConfig: (cwd?: string) => Promise<ResolvedConfigRoots>
}

type JsonConfigReadOptions = {
  validateNormalized?: (normalized: unknown) => void
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

/** Reads and validates one JSON config document when the file exists. */
export async function readJsonConfig<T>(
  path: string,
  schema: z.ZodType<T>,
  label: string,
  schemaReference: string,
  options: JsonConfigReadOptions = {},
): Promise<T | undefined> {
  if (!(await pathExists(path))) {
    return undefined
  }

  let parsed: unknown

  try {
    parsed = JSON.parse(await readFile(path, "utf-8"))
  } catch (error) {
    throw new Error(`${label} at ${path} must be valid JSON.`, {
      cause: error,
    })
  }

  if (typeof parsed === "object" && parsed !== null && !("$schema" in parsed)) {
    try {
      await writeFile(
        path,
        `${JSON.stringify(
          {
            ...parsed,
            $schema: new URL(schemaReference, SCHEMA_BASE_URL).toString(),
          },
          null,
          2,
        )}\n`,
        "utf-8",
      )
    } catch {
      // Best-effort normalization only.
    }
  }

  const normalized =
    typeof parsed === "object" && parsed !== null && "$schema" in parsed
      ? Object.fromEntries(
          Object.entries(parsed as Record<string, unknown>).filter(([key]) => key !== "$schema"),
        )
      : parsed

  try {
    options.validateNormalized?.(normalized)
  } catch (error) {
    throw new Error(
      `${label} at ${path} is invalid: ${error instanceof Error ? error.message : String(error)}`,
      { cause: error },
    )
  }

  const result = schema.safeParse(normalized)
  if (!result.success) {
    throw new Error(`${label} at ${path} is invalid: ${z.prettifyError(result.error)}`)
  }

  return result.data
}

/** Reads and merges the global and local root config documents for one working directory. */
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
      await readJsonConfig(getLocalConfigPath(cwd), UserConfig, "Local config", "goddard.json", {
        validateNormalized: assertLocalConfigDoesNotDeclareWorktreePlugins,
      }),
    ),
  }
}

/**
 * Prevents repository-local config from selecting arbitrary daemon-loaded worktree plugins.
 */
function assertLocalConfigDoesNotDeclareWorktreePlugins(normalized: unknown) {
  if (typeof normalized !== "object" || normalized === null || Array.isArray(normalized)) {
    return
  }

  const worktrees = (normalized as Record<string, unknown>).worktrees
  if (typeof worktrees !== "object" || worktrees === null || Array.isArray(worktrees)) {
    return
  }

  if ("plugins" in worktrees) {
    throw new Error(
      "`worktrees.plugins` is only supported in the global Goddard config, not repository-local config.",
    )
  }
}

/** Reads the current merged root config from one provider when available, otherwise from disk. */
export async function readCurrentRootConfig(cwd: string, provider?: RootConfigProvider) {
  if (!provider) {
    return readMergedRootConfig(cwd)
  }

  const snapshot = await provider.getRootConfig(cwd)
  return {
    globalRoot: snapshot.globalRoot,
    localRoot: snapshot.localRoot,
    config: snapshot.config,
  }
}

/** Reads and validates one packaged action config document. */
export async function readActionConfig(path: string): Promise<ActionConfig | undefined> {
  return readJsonConfig(path, ActionConfig, "Action config", "action.json")
}

/** Reads and validates one packaged loop config document. */
export async function readLoopConfig(path: string): Promise<LoopConfig | undefined> {
  return readJsonConfig(path, LoopConfig, "Loop config", "loop.json")
}
