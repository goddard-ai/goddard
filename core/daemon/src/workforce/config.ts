import type { WorkforceAgentConfig, WorkforceConfig } from "@goddard-ai/schema/workforce"
import { execFile } from "node:child_process"
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises"
import { basename, join, relative, resolve } from "node:path"
import { promisify } from "node:util"
import { buildWorkforcePaths } from "./paths.ts"

const execFileAsync = promisify(execFile)

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

/** Checks whether a parsed JSON value is a plain object record. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && Array.isArray(value) === false
}

/** Validates one agent entry loaded from the repo-local workforce config file. */
function assertAgentConfig(value: unknown, index: number): asserts value is WorkforceAgentConfig {
  if (!isRecord(value)) {
    throw new Error(`Invalid workforce agent at index ${index}`)
  }

  if (typeof value.id !== "string" || value.id.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty id`)
  }

  if (typeof value.name !== "string" || value.name.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty name`)
  }

  if (value.role !== "root" && value.role !== "domain") {
    throw new Error(`Workforce agent ${index} has an invalid role`)
  }

  if (typeof value.cwd !== "string" || value.cwd.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty cwd`)
  }

  if (
    Array.isArray(value.owns) === false ||
    value.owns.some((entry) => typeof entry !== "string" || entry.length === 0)
  ) {
    throw new Error(`Workforce agent ${index} must include non-empty owned paths`)
  }
}

/** Reads a package name from one manifest or falls back to the directory name. */
async function resolvePackageName(manifestPath: string, packageDir: string): Promise<string> {
  try {
    const parsed = JSON.parse(await readFile(manifestPath, "utf-8")) as { name?: string }
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
  return {
    rootDir: packageDir,
    relativeDir: relative(rootDir, packageDir) || ".",
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
function buildInitializedWorkforceConfig(packages: DiscoveredWorkforcePackage[]): WorkforceConfig {
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
    defaultAgent: "pi-acp",
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
  const parsed = JSON.parse(await readFile(paths.configPath, "utf-8")) as unknown

  if (!isRecord(parsed) || parsed.version !== 1) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  if (
    (typeof parsed.defaultAgent !== "string" && isRecord(parsed.defaultAgent) === false) ||
    typeof parsed.rootAgentId !== "string" ||
    Array.isArray(parsed.agents) === false
  ) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  parsed.agents.forEach((agent, index) => {
    assertAgentConfig(agent, index)
  })

  if (parsed.agents.some((agent) => agent.id === parsed.rootAgentId) === false) {
    throw new Error(`Workforce config at ${paths.configPath} must include the root agent`)
  }

  return parsed as unknown as WorkforceConfig
}

/** Ensures the workforce config directory and append-only ledger file exist. */
export async function ensureWorkforceFiles(rootDir: string): Promise<void> {
  const paths = buildWorkforcePaths(rootDir)
  await mkdir(paths.goddardDir, { recursive: true })

  try {
    await readFile(paths.ledgerPath, "utf-8")
  } catch {
    await writeFile(paths.ledgerPath, "", "utf-8")
  }
}

/** Resolves the nearest git repository root from one starting directory. */
export async function resolveRepositoryRoot(startDir: string): Promise<string> {
  try {
    const { stdout } = await execFileAsync("git", ["rev-parse", "--show-toplevel"], {
      cwd: resolve(startDir),
    })
    return stdout.trim()
  } catch (error) {
    throw new Error(
      `Unable to resolve the repository root from ${resolve(startDir)}: ${
        error instanceof Error ? error.message : String(error)
      }`,
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
): Promise<InitializedWorkforce> {
  const repositoryRoot = resolve(rootDir)
  const packages = await Promise.all(
    packageDirs.map((packageDir) => toDiscoveredPackage(repositoryRoot, packageDir)),
  )
  const paths = buildWorkforcePaths(repositoryRoot)
  const createdPaths: string[] = []

  await mkdir(paths.goddardDir, { recursive: true })

  const config = buildInitializedWorkforceConfig(packages)
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
