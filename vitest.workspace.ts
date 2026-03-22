import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, relative, resolve } from "node:path"
import type { UserWorkspaceConfig } from "vitest/config"

/**
 * Minimal package.json shape needed to derive local-package source aliases.
 */
type PackageManifest = {
  name?: string
  exports?: Record<string, unknown>
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
}

/**
 * One package project that can contribute test files and source aliases.
 */
type WorkspacePackage = {
  dir: string
  manifest: PackageManifest
  name: string
}

/**
 * Regex-based alias entry accepted by Vite and Vitest workspace configs.
 */
type AliasEntry = {
  find: RegExp
  replacement: string
}

const ignoredDirectoryNames = new Set([".git", "dist", "node_modules"])
const rootDir = resolve(import.meta.dirname)
const resolvedWorkspacePackageCache = new Map<string, WorkspacePackage>()
const workspacePackages = readWorkspacePackages()

/**
 * Project definitions consumed by the root Vitest config.
 */
export const workspaceProjects = workspacePackages.map(
  (pkg): UserWorkspaceConfig => ({
    resolve: {
      alias: createWorkspaceAliases(pkg),
      conditions: ["source", "import", "default"],
    },
    test: {
      name: pkg.name,
      environment: "node" as const,
      include: [`${relative(rootDir, pkg.dir)}/**/*.test.ts`],
      globals: true,
    },
  }),
)

/**
 * Scans the repository for package roots instead of duplicating pnpm workspace parsing.
 */
function readWorkspacePackages(): WorkspacePackage[] {
  return findPackageDirectories(rootDir)
    .map(readWorkspacePackage)
    .filter((pkg): pkg is WorkspacePackage => pkg !== null)
    .sort((left, right) => left.name.localeCompare(right.name))
}

/**
 * Recursively finds package directories beneath the repo root while skipping generated folders.
 */
function findPackageDirectories(dir: string): string[] {
  const packageDirs: string[] = []

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory() || ignoredDirectoryNames.has(entry.name)) {
      continue
    }

    const childDir = join(dir, entry.name)
    if (existsSync(join(childDir, "package.json"))) {
      packageDirs.push(childDir)
    }

    packageDirs.push(...findPackageDirectories(childDir))
  }

  return packageDirs
}

/**
 * Loads one named package manifest from disk.
 */
function readWorkspacePackage(dir: string): WorkspacePackage | null {
  const manifestPath = join(dir, "package.json")
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as PackageManifest

  if (typeof manifest.name !== "string" || manifest.name.length === 0) {
    return null
  }

  return {
    dir,
    manifest,
    name: manifest.name,
  }
}

/**
 * Builds source aliases for one project package and its reachable local workspace dependencies.
 */
function createWorkspaceAliases(pkg: WorkspacePackage): AliasEntry[] {
  const aliases: AliasEntry[] = []
  const reachablePackages = [pkg, ...collectReachableWorkspaceDependencies(pkg)]

  for (const dependency of reachablePackages) {
    aliases.push(...createSourceAliasesForPackage(dependency))
  }

  console.log("Created aliases for package", pkg.name, aliases)

  return aliases
}

/**
 * Walks the local dependency graph by following workspace protocol entries transitively.
 */
function collectReachableWorkspaceDependencies(pkg: WorkspacePackage): WorkspacePackage[] {
  const resolvedPackages: WorkspacePackage[] = []
  const visited = new Set<string>()
  const queue = getLocalWorkspaceDependencyNames(pkg.manifest).map((packageName) => ({
    fromDir: pkg.dir,
    packageName,
  }))

  while (queue.length > 0) {
    const dependencyRequest = queue.shift()
    if (!dependencyRequest || visited.has(dependencyRequest.packageName)) {
      continue
    }

    visited.add(dependencyRequest.packageName)

    const dependency = resolveWorkspacePackage(
      dependencyRequest.packageName,
      dependencyRequest.fromDir,
    )
    resolvedPackages.push(dependency)
    queue.push(
      ...getLocalWorkspaceDependencyNames(dependency.manifest).map((packageName) => ({
        fromDir: dependency.dir,
        packageName,
      })),
    )
  }

  return resolvedPackages.sort((left, right) => left.name.localeCompare(right.name))
}

/**
 * Reads one package's direct local dependencies based on the workspace protocol.
 */
function getLocalWorkspaceDependencyNames(manifest: PackageManifest): string[] {
  const dependencyMap = {
    ...manifest.dependencies,
    ...manifest.devDependencies,
  }

  return Object.entries(dependencyMap)
    .filter(([, version]) => version.startsWith("workspace:"))
    .map(([dependencyName]) => dependencyName)
    .sort((left, right) => left.localeCompare(right))
}

/**
 * Resolves a local package through the declaring package and then walks upward to the owning package root.
 */
function resolveWorkspacePackage(packageName: string, fromDir: string): WorkspacePackage {
  const cachedPackage = resolvedWorkspacePackageCache.get(packageName)
  if (cachedPackage) {
    return cachedPackage
  }

  const packageEntryPath = resolveWorkspacePackageEntryPath(packageName, fromDir)
  const packageDir = findNearestPackageDirectory(packageEntryPath)
  const pkg = readWorkspacePackage(packageDir)

  if (!pkg || pkg.name !== packageName) {
    throw new Error(`Resolved workspace package root does not match ${packageName}: ${packageDir}`)
  }

  resolvedWorkspacePackageCache.set(packageName, pkg)
  return pkg
}

/**
 * Resolves the filesystem entry for one local package, including packages that only export subpaths.
 */
function resolveWorkspacePackageEntryPath(packageName: string, fromDir: string): string {
  const packageRequire = createRequire(join(fromDir, "package.json"))

  try {
    return packageRequire.resolve(packageName)
  } catch (error) {
    if (!isPackageEntryResolutionError(error)) {
      throw error
    }
  }

  const linkedPackageDir = resolve(fromDir, "node_modules", packageName)
  if (existsSync(linkedPackageDir)) {
    return realpathSync(linkedPackageDir)
  }

  throw new Error(`Could not resolve workspace package ${packageName}`)
}

/**
 * Searches upward from one resolved module path until a package.json boundary is found.
 */
function findNearestPackageDirectory(resolvedPath: string): string {
  let currentDir = existsSync(join(resolvedPath, "package.json"))
    ? resolvedPath
    : dirname(resolvedPath)

  while (true) {
    if (existsSync(join(currentDir, "package.json"))) {
      return currentDir
    }

    const parentDir = dirname(currentDir)
    if (parentDir === currentDir) {
      throw new Error(`Could not find package.json above ${resolvedPath}`)
    }

    currentDir = parentDir
  }
}

/**
 * Narrows resolution failures that mean the package exists but has no bare export entry.
 */
function isPackageEntryResolutionError(error: unknown): boolean {
  if (!(error instanceof Error) || !("code" in error)) {
    return false
  }

  return error.code === "ERR_PACKAGE_PATH_NOT_EXPORTED" || error.code === "MODULE_NOT_FOUND"
}

/**
 * Converts one package export map into concrete source aliases.
 */
function createSourceAliasesForPackage(pkg: WorkspacePackage): AliasEntry[] {
  const exportsMap = pkg.manifest.exports
  if (!exportsMap) {
    return []
  }

  const aliases: AliasEntry[] = []

  for (const [subpath, value] of Object.entries(exportsMap)) {
    const sourceTarget = readSourceExportTarget(value)
    if (!sourceTarget) {
      continue
    }

    const absoluteTarget = resolve(pkg.dir, sourceTarget)
    if (subpath === ".") {
      aliases.push({
        find: new RegExp(`^${escapeRegex(pkg.name)}$`),
        replacement: absoluteTarget,
      })
      continue
    }

    if (!subpath.startsWith("./")) {
      continue
    }

    const specifierPattern = `${pkg.name}${subpath.slice(1)}`
    if (subpath.includes("*")) {
      aliases.push({
        find: new RegExp(`^${escapeRegex(specifierPattern).replace(/\\\*/g, "(.+)")}$`),
        replacement: absoluteTarget.replaceAll("*", "$1"),
      })
      continue
    }

    aliases.push({
      find: new RegExp(`^${escapeRegex(specifierPattern)}$`),
      replacement: absoluteTarget,
    })
  }

  return aliases
}

/**
 * Reads the `source` export target from one package export entry when present.
 */
function readSourceExportTarget(value: unknown): string | null {
  if (!value || typeof value !== "object") {
    return null
  }

  const source = (value as Record<string, unknown>).source
  return typeof source === "string" ? source : null
}

/**
 * Escapes one string before it is embedded in a regular expression.
 */
function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
