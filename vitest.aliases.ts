import { existsSync, readFileSync, readdirSync } from "node:fs"
import { join, resolve } from "node:path"

/**
 * Minimal package.json shape needed to derive source aliases from workspace exports.
 */
type PackageManifest = {
  name?: string
  exports?: Record<string, unknown>
}

/**
 * One workspace package discovered from the repository tree.
 */
type WorkspacePackage = {
  dir: string
  manifest: PackageManifest
  name: string
}

/**
 * Regex-based alias entry accepted by Vite and Vitest.
 */
type AliasEntry = {
  find: RegExp
  replacement: string
}

const ignoredDirectoryNames = new Set([".git", "dist", "node_modules"])
const rootDir = resolve(import.meta.dirname)

/**
 * Global source aliases for every workspace package export that declares a `types.source` target.
 */
export const workspaceAliases = readWorkspacePackages().flatMap(createSourceAliasesForPackage)

/**
 * Scans the repository for package roots instead of maintaining a package list by hand.
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
 * Reads the `types.source` export target from one package export entry when present.
 */
function readSourceExportTarget(value: unknown): string | null {
  if (!value || typeof value !== "object") {
    return null
  }

  const types = (value as Record<string, unknown>).types
  if (!types || typeof types !== "object") {
    return null
  }

  const source = (types as Record<string, unknown>).source
  return typeof source === "string" ? source : null
}

/**
 * Escapes one string before it is embedded in a regular expression.
 */
function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
