import { constants as fsConstants } from "node:fs"
import { access, readFile, writeFile } from "node:fs/promises"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import { getErrorMessage } from "radashi"
import { z } from "zod"

import { buildRootConfigSchema, mergeRootConfigLayers } from "../config-schema.ts"

export type RootConfig = Record<string, any>

/** Paths and merged root config for one daemon-side config resolution request. */
export type ResolvedConfigRoots = {
  globalRoot: string
  localRoot: string
  config: RootConfig
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
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

  if (isRecord(parsed) && !("$schema" in parsed)) {
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

  const normalized = isRecord(parsed)
    ? Object.fromEntries(Object.entries(parsed).filter(([key]) => key !== "$schema"))
    : parsed

  try {
    options.validateNormalized?.(normalized)
  } catch (error) {
    throw new Error(`${label} at ${path} is invalid: ${getErrorMessage(error)}`, { cause: error })
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
  const rootConfigSchema = buildRootConfigSchema()
  const user = await readJsonConfig(
    getGlobalConfigPath(),
    rootConfigSchema,
    "Global config",
    "goddard.json",
  )
  const project = await readJsonConfig(
    getLocalConfigPath(cwd),
    rootConfigSchema,
    "Local config",
    "goddard.json",
    {
      validateNormalized: assertLocalConfigIsWithinSupportedScope,
    },
  )

  return {
    globalRoot,
    localRoot,
    config: await mergeRootConfigLayers(user, project),
  }
}

/**
 * Prevents repository-local config from declaring daemon-owned global-only settings.
 */
function assertLocalConfigIsWithinSupportedScope(normalized: unknown) {
  if (!isRecord(normalized)) {
    return
  }

  const config = normalized
  if ("daemon" in config) {
    throw new Error(
      "`daemon` is only supported in the global Goddard config, not repository-local config.",
    )
  }

  const worktrees = config.worktrees
  if (isRecord(worktrees) && "plugins" in worktrees) {
    throw new Error(
      "`worktrees.plugins` is only supported in the global Goddard config, not repository-local config.",
    )
  }

  const sessions = config.sessions
  const envPolicy = isRecord(sessions) ? sessions.envPolicy : null
  if (isRecord(envPolicy) && "set" in envPolicy) {
    throw new Error(
      "`sessions.envPolicy.set` is only supported in the global Goddard config, not repository-local config.",
    )
  }

  const security = config.security
  const pullRequests = isRecord(security) ? security.pullRequests : null
  if (isRecord(pullRequests)) {
    for (const key of ["submit", "reply"]) {
      if (pullRequests[key] === "allow") {
        throw new Error(
          `\`security.pullRequests.${key}\` cannot be set to "allow" in repository-local config.`,
        )
      }
    }
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
