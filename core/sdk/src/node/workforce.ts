import type { AgentDistribution } from "@goddard-ai/schema/session-server"
import type { WorkforceConfig } from "@goddard-ai/schema/workforce"
import { execFile } from "node:child_process"
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises"
import { basename, join, relative, resolve } from "node:path"
import { promisify } from "node:util"
export {
  cancelDaemonWorkforceRequest as cancelWorkforceRequest,
  createDaemonWorkforceRequest as createWorkforceRequest,
  getDaemonWorkforce as getWorkforce,
  listDaemonWorkforces as listWorkforces,
  startDaemonWorkforce as startWorkforce,
  shutdownDaemonWorkforce as stopWorkforce,
  truncateDaemonWorkforce as truncateWorkforce,
  updateDaemonWorkforceRequest as updateWorkforceRequest,
  type WorkforceClientOptions,
} from "../daemon/workforce.js"

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

function sanitizeAgentId(value: string): string {
  return value
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase()
}

function buildWorkforceConfig(
  packages: DiscoveredWorkforcePackage[],
  defaultAgent: string | AgentDistribution = "pi-acp",
): WorkforceConfig {
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
    defaultAgent,
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

export async function discoverWorkforceInitCandidates(
  rootDir: string,
): Promise<DiscoveredWorkforcePackage[]> {
  return Promise.all(
    (await discoverNestedPackageDirs(rootDir)).map((packageDir) =>
      toDiscoveredPackage(rootDir, packageDir),
    ),
  )
}

export async function initializeWorkforce(
  rootDir: string,
  packageDirs: string[],
): Promise<InitializedWorkforce> {
  const repositoryRoot = resolve(rootDir)
  const packages = await Promise.all(
    packageDirs.map((packageDir) => toDiscoveredPackage(repositoryRoot, packageDir)),
  )
  const goddardDir = join(repositoryRoot, ".goddard")
  const configPath = join(goddardDir, "workforce.json")
  const ledgerPath = join(goddardDir, "ledger.jsonl")
  const createdPaths: string[] = []

  await mkdir(goddardDir, { recursive: true })

  const config = buildWorkforceConfig(packages)
  await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`, "utf-8")
  createdPaths.push(configPath)

  try {
    const existing = await stat(ledgerPath)
    if (existing.isFile() === false) {
      await writeFile(ledgerPath, "", "utf-8")
      createdPaths.push(ledgerPath)
    }
  } catch {
    await writeFile(ledgerPath, "", "utf-8")
    createdPaths.push(ledgerPath)
  }

  return {
    rootDir: repositoryRoot,
    configPath,
    ledgerPath,
    createdPaths,
  }
}
