/** Daemon-owned loader for custom worktree plugins declared in global config. */
import type { WorktreePluginReference } from "@goddard-ai/schema/config"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"
import hashSum from "hash-sum"
import { access } from "node:fs/promises"
import * as path from "node:path"
import { pathToFileURL } from "node:url"
import type { ConfigManager, RootConfigSnapshot } from "../config-manager.ts"
import { createLogger } from "../logging.ts"

const builtinWorktreePluginNames = new Set(["default", "worktrunk"])
type LoadedWorktreePluginSet = {
  signature: string
  plugins: WorktreePlugin[]
}

/** Daemon-owned contract for loading custom worktree plugins from root config. */
export interface WorktreePluginManager {
  getPlugins: (cwd: string) => Promise<WorktreePlugin[]>
}

/**
 * Creates a loader that resolves configured custom worktree plugins from the global config.
 */
export function createWorktreePluginManager(input: {
  configManager: ConfigManager
  logger?: ReturnType<typeof createLogger>
}) {
  const logger = input.logger ?? createLogger()
  const lastSuccessfulLoads = new Map<string, LoadedWorktreePluginSet>()

  async function getPlugins(cwd: string) {
    const snapshot = await input.configManager.getRootConfig(cwd)
    const references = snapshot.config.worktrees?.plugins ?? []
    const cacheKey = createPluginSetCacheKey(snapshot)
    const previousLoad = lastSuccessfulLoads.get(cacheKey) ?? null
    const signature = hashSum(references)

    if (previousLoad && previousLoad.signature === signature) {
      return previousLoad.plugins
    }

    try {
      const plugins = await loadConfiguredWorktreePlugins({
        references,
        snapshot,
      })
      lastSuccessfulLoads.set(cacheKey, { signature, plugins })
      return plugins
    } catch (error) {
      if (previousLoad) {
        logger.log("worktree.plugins_load_failed", {
          cwd,
          globalRoot: snapshot.globalRoot,
          usingLastGoodPlugins: true,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
        return previousLoad.plugins
      }

      throw error
    }
  }

  return {
    getPlugins,
  } satisfies WorktreePluginManager
}

/**
 * Loads and validates one configured worktree plugin set from the current config snapshot.
 */
async function loadConfiguredWorktreePlugins(params: {
  references: WorktreePluginReference[]
  snapshot: RootConfigSnapshot
}) {
  const plugins: WorktreePlugin[] = []
  const seenNames = new Set<string>()

  for (const reference of params.references) {
    const plugin = await loadConfiguredWorktreePlugin({
      reference,
      snapshot: params.snapshot,
    })

    if (builtinWorktreePluginNames.has(plugin.name)) {
      throw new Error(`Configured worktree plugin "${plugin.name}" uses a reserved built-in name.`)
    }

    if (seenNames.has(plugin.name)) {
      throw new Error(`Configured worktree plugin name "${plugin.name}" must be unique.`)
    }

    seenNames.add(plugin.name)
    plugins.push(plugin)
  }

  return plugins
}

/**
 * Loads one configured worktree plugin module and extracts the requested export.
 */
async function loadConfiguredWorktreePlugin(params: {
  reference: WorktreePluginReference
  snapshot: RootConfigSnapshot
}) {
  const moduleNamespace =
    params.reference.type === "path"
      ? await importConfiguredPathPlugin(params.reference, params.snapshot)
      : await importConfiguredPackagePlugin(params.reference)
  const exportName = params.reference.export ?? "default"
  const plugin = moduleNamespace[exportName]

  return assertWorktreePlugin(plugin, describeWorktreePluginReference(params.reference, exportName))
}

/**
 * Imports one configured plugin module from a filesystem path.
 */
async function importConfiguredPathPlugin(
  reference: Extract<WorktreePluginReference, { type: "path" }>,
  snapshot: RootConfigSnapshot,
) {
  const resolvedPath = path.isAbsolute(reference.path)
    ? path.resolve(reference.path)
    : path.resolve(snapshot.globalRoot, reference.path)

  await assertPathExists(
    resolvedPath,
    `Configured worktree plugin path does not exist: ${reference.path}`,
  )

  return importVersionedModule(resolvedPath, snapshot.version)
}

/**
 * Imports one configured plugin module from a package specifier resolved by the runtime.
 */
async function importConfiguredPackagePlugin(
  reference: Extract<WorktreePluginReference, { type: "package" }>,
) {
  try {
    return await import(reference.package)
  } catch (error) {
    throw new Error(`Unable to import configured worktree plugin package "${reference.package}".`, {
      cause: error,
    })
  }
}

/**
 * Imports one plugin module URL with a config-version query so config updates reload it cleanly.
 */
async function importVersionedModule(modulePath: string, version: number) {
  const moduleUrl = pathToFileURL(modulePath)
  moduleUrl.searchParams.set("v", String(version))
  return import(moduleUrl.href)
}

/**
 * Verifies that an arbitrary module export satisfies the runtime worktree plugin contract.
 */
function assertWorktreePlugin(value: unknown, sourceDescription: string) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${sourceDescription} must export a worktree plugin object.`)
  }

  const plugin = value as Partial<WorktreePlugin>
  if (typeof plugin.name !== "string" || plugin.name.length === 0) {
    throw new Error(`${sourceDescription} must define a non-empty string \`name\`.`)
  }
  if (typeof plugin.isApplicable !== "function") {
    throw new Error(`${sourceDescription} must define an \`isApplicable\` function.`)
  }
  if (typeof plugin.setup !== "function") {
    throw new Error(`${sourceDescription} must define a \`setup\` function.`)
  }
  if (typeof plugin.cleanup !== "function") {
    throw new Error(`${sourceDescription} must define a \`cleanup\` function.`)
  }

  return plugin as WorktreePlugin
}

/**
 * Builds one stable description string for config-loaded plugin errors.
 */
function describeWorktreePluginReference(reference: WorktreePluginReference, exportName: string) {
  if (reference.type === "path") {
    return `Configured worktree plugin path "${reference.path}" export "${exportName}"`
  }

  return `Configured worktree plugin package "${reference.package}" export "${exportName}"`
}

/**
 * Uses the config snapshot root as the cache partition for one daemon-wide plugin registry.
 */
function createPluginSetCacheKey(snapshot: RootConfigSnapshot) {
  return path.resolve(snapshot.globalRoot)
}

/**
 * Returns true when one filesystem path exists.
 */
async function pathExists(targetPath: string) {
  try {
    await access(targetPath)
    return true
  } catch {
    return false
  }
}

/**
 * Throws when a required filesystem path does not exist.
 */
async function assertPathExists(targetPath: string, message: string) {
  if (!(await pathExists(targetPath))) {
    throw new Error(message)
  }
}
