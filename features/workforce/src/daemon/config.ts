import { mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises"
import { basename, join, relative, resolve } from "node:path"
import { resolveDefaultAgent } from "@goddard-ai/config/node"
import { createGitHost } from "@goddard-ai/git"
import { getErrorMessage, isObject } from "radashi"

import type { WorkforceAgentConfig, WorkforceConfig } from "../schema.ts"
import { buildWorkforcePaths, normalizeWorkforceRootDir } from "./paths.ts"

// Common directory names skipped during workspace package discovery.
const IGNORED_DIRECTORY_NAMES = new Set([".git", "dist", "node_modules"])

/** Package metadata discovered from nested package manifests under a repository root. */
export type DiscoveredWorkforcePackage = {
  rootDir: string
  relativeDir: string
  manifestPath: string
  name: string
}

/** Result metadata returned after initializing repository workforce files. */
export type InitializedWorkforce = {
  rootDir: string
  configPath: string
  ledgerPath: string
  createdPaths: string[]
}

/** Validates one agent entry loaded from the repo-local workforce config file. */
function assertAgentConfig(value: unknown, index: number): asserts value is WorkforceAgentConfig {
  if (!isObject(value)) {
    throw new Error(`Invalid workforce agent at index ${index}`)
  }

  const record = value as Record<string, unknown>

  if (typeof record.id !== "string" || record.id.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty id`)
  }

  if (typeof record.name !== "string" || record.name.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty name`)
  }

  if (record.role !== "root" && record.role !== "domain") {
    throw new Error(`Workforce agent ${index} has an invalid role`)
  }

  if (typeof record.cwd !== "string" || record.cwd.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty cwd`)
  }

  if (
    Array.isArray(record.owns) === false ||
    record.owns.some((entry) => typeof entry !== "string" || entry.length === 0)
  ) {
    throw new Error(`Workforce agent ${index} must include non-empty owned paths`)
  }
}

/** Reads a package name from one manifest or falls back to the directory name. */
async function resolvePackageName(manifestPath: string, packageDir: string): Promise<string> {
  try {
    const parsed = JSON.parse(await readFile(manifestPath, "utf-8")) as {
      name?: string
    }
    if (typeof parsed.name === "string" && parsed.name.trim()) {
      return parsed.name
    }
  } catch {
    // Fall back to the directory name when the manifest cannot be parsed.
  }

  return basename(packageDir)
}

/** Walks the repository tree while skipping ignored directories. */
async function walkDirectory(
  directory: string,
  visitor: (entryPath: string) => Promise<void>,
): Promise<void> {
  const entries = await readdir(directory, { withFileTypes: true })

  for (const entry of entries) {
    if (!entry.isDirectory() || IGNORED_DIRECTORY_NAMES.has(entry.name)) {
      continue
    }

    const entryPath = join(directory, entry.name)
    await visitor(entryPath)
    await walkDirectory(entryPath, visitor)
  }
}

/** Discovers package roots nested under one repository root. */
async function discoverNestedPackageDirs(rootDir: string): Promise<string[]> {
  const resolvedRootDir = resolve(rootDir)
  const packageDirs: string[] = []

  try {
    const rootManifestStats = await stat(join(resolvedRootDir, "package.json"))
    if (rootManifestStats.isFile()) {
      packageDirs.push(resolvedRootDir)
    }
  } catch {
    // Ignore repositories without a root package manifest.
  }

  await walkDirectory(resolvedRootDir, async (entryPath) => {
    try {
      const manifestStats = await stat(join(entryPath, "package.json"))
      if (manifestStats.isFile()) {
        packageDirs.push(entryPath)
      }
    } catch {
      // Ignore directories that are not package roots.
    }
  })

  return packageDirs.sort()
}

/** Converts one package directory into the CLI's discovery shape. */
async function toDiscoveredPackage(
  rootDir: string,
  packageDir: string,
): Promise<DiscoveredWorkforcePackage> {
  const relativeDir = relative(rootDir, packageDir).replaceAll("\\", "/") || "."

  return {
    rootDir: packageDir,
    relativeDir,
    manifestPath: join(packageDir, "package.json"),
    name: await resolvePackageName(join(packageDir, "package.json"), packageDir),
  }
}

/** Normalizes a package name into a stable workforce agent id. */
function sanitizeAgentId(value: string): string {
  return value
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase()
}

/** Builds the initial repo-local workforce config for the selected packages. */
async function buildInitializedWorkforceConfig(
  packages: DiscoveredWorkforcePackage[],
  input: { defaultAgent?: WorkforceConfig["defaultAgent"] } = {},
): Promise<WorkforceConfig> {
  const domainAgents = packages
    .filter((pkg) => pkg.relativeDir !== ".")
    .map((pkg) => ({
      id: sanitizeAgentId(pkg.name || pkg.relativeDir),
      name: pkg.name,
      role: "domain" as const,
      cwd: pkg.relativeDir,
      owns: [pkg.relativeDir],
    }))

  return {
    version: 1,
    defaultAgent: input.defaultAgent ?? (await resolveDefaultAgent()),
    rootAgentId: "root",
    agents: [
      {
        id: "root",
        name: "@repo/root",
        role: "root",
        cwd: ".",
        owns: ["."],
      },
      ...domainAgents,
    ],
  }
}

/** Reads and validates one repo-local workforce config file. */
export async function readWorkforceConfig(rootDir: string): Promise<WorkforceConfig> {
  const paths = buildWorkforcePaths(rootDir)
  const parsed = JSON.parse(await Bun.file(paths.configPath).text()) as unknown

  if (!isObject(parsed)) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  const record = parsed as Record<string, unknown>

  if (record.version !== 1) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  if (
    (typeof record.defaultAgent !== "string" && isObject(record.defaultAgent) === false) ||
    typeof record.rootAgentId !== "string" ||
    Array.isArray(record.agents) === false
  ) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  record.agents.forEach((agent, index) => {
    assertAgentConfig(agent, index)
  })

  if (record.agents.some((agent) => agent.id === record.rootAgentId) === false) {
    throw new Error(`Workforce config at ${paths.configPath} must include the root agent`)
  }

  return parsed as unknown as WorkforceConfig
}

/** Ensures the workforce config directory and append-only ledger file exist. */
export async function ensureWorkforceFiles(rootDir: string): Promise<void> {
  const paths = buildWorkforcePaths(rootDir)
  await mkdir(paths.goddardDir, { recursive: true })

  try {
    await Bun.file(paths.ledgerPath).text()
  } catch {
    await Bun.write(paths.ledgerPath, "")
  }
}

/** Resolves the nearest git repository root from one starting directory. */
export async function resolveRepositoryRoot(startDir: string): Promise<string> {
  try {
    const rootDir = await createGitHost().repository.resolveRoot(resolve(startDir))
    return await normalizeWorkforceRootDir(rootDir)
  } catch (error) {
    throw new Error(
      `Unable to resolve the repository root from ${resolve(startDir)}: ${getErrorMessage(error)}`,
    )
  }
}

/** Discovers workforce initialization candidates under one repository root. */
export async function discoverWorkforceInitCandidates(
  rootDir: string,
): Promise<DiscoveredWorkforcePackage[]> {
  return Promise.all(
    (await discoverNestedPackageDirs(rootDir)).map((packageDir) =>
      toDiscoveredPackage(rootDir, packageDir),
    ),
  )
}

/** Writes the initial workforce config and ledger files into `.goddard`. */
export async function initializeWorkforce(
  rootDir: string,
  packageDirs: string[],
  options: { defaultAgent?: WorkforceConfig["defaultAgent"] } = {},
): Promise<InitializedWorkforce> {
  const repositoryRoot = resolve(rootDir)
  const packages = await Promise.all(
    packageDirs.map((packageDir) => toDiscoveredPackage(repositoryRoot, packageDir)),
  )
  const paths = buildWorkforcePaths(repositoryRoot)
  const createdPaths: string[] = []

  await mkdir(paths.goddardDir, { recursive: true })

  const config = await buildInitializedWorkforceConfig(packages, {
    defaultAgent: options.defaultAgent,
  })
  await writeFile(paths.configPath, `${JSON.stringify(config, null, 2)}\n`, "utf-8")
  createdPaths.push(paths.configPath)

  try {
    const existing = await stat(paths.ledgerPath)
    if (existing.isFile() === false) {
      await writeFile(paths.ledgerPath, "", "utf-8")
      createdPaths.push(paths.ledgerPath)
    }
  } catch {
    await writeFile(paths.ledgerPath, "", "utf-8")
    createdPaths.push(paths.ledgerPath)
  }

  return {
    rootDir: repositoryRoot,
    configPath: paths.configPath,
    ledgerPath: paths.ledgerPath,
    createdPaths,
  }
}
