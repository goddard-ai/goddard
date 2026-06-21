/** Discovers package-boundary subdirectories that can be selected as session working directories. */
import { type Dirent } from "node:fs"
import { readdir, realpath, stat } from "node:fs/promises"
import { basename, isAbsolute, join, relative, resolve } from "node:path"
import { runGitCommand } from "@goddard-ai/git"

import type { SessionSubpackage } from "../schema.ts"
import { resolveGitRepoRoot } from "./worktree.ts"

const BUILT_IN_SUBPACKAGE_MANIFESTS = [
  "package.json",
  "pyproject.toml",
  "Cargo.toml",
  "go.mod",
  "deno.json",
]
const SUBPACKAGE_IGNORED_DIRECTORY_NAMES = new Set([
  "node_modules",
  "dist",
  "build",
  "coverage",
  "target",
])

/** Reads one directory with string entry names across Node and Bun. */
async function readDirectoryEntries(path: string) {
  return (await readdir(path, {
    encoding: "utf-8",
    withFileTypes: true,
  })) as Dirent<string>[]
}

/** Sorts directory entries so folders stay ahead of files and names remain deterministic. */
function sortDirectoryEntries(entries: readonly Dirent<string>[]) {
  return [...entries].sort((left, right) => {
    if (left.isDirectory() !== right.isDirectory()) {
      return left.isDirectory() ? -1 : 1
    }

    return left.name.localeCompare(right.name)
  })
}

/** Merges built-in package-boundary manifests with user-configured additions. */
function resolveSubpackageManifestEntries(configuredEntries: readonly string[] | undefined) {
  const manifests: string[] = []
  const seen = new Set<string>()

  for (const entry of [...BUILT_IN_SUBPACKAGE_MANIFESTS, ...(configuredEntries ?? [])]) {
    const normalized = entry.trim().replaceAll("\\", "/").replace(/^\.\//, "")
    const segments = normalized.split("/")

    if (
      normalized.length === 0 ||
      isAbsolute(normalized) ||
      segments.some((segment) => segment.length === 0 || segment === "." || segment === "..")
    ) {
      continue
    }

    if (seen.has(normalized)) {
      continue
    }

    seen.add(normalized)
    manifests.push(normalized)
  }

  return manifests
}

/** Returns true when a directory name should never be scanned for subpackage boundaries. */
function isIgnoredSubpackageDirectoryName(name: string) {
  return name.startsWith(".") || SUBPACKAGE_IGNORED_DIRECTORY_NAMES.has(name)
}

/** Counts relative path segments so shallow subpackages can sort ahead of nested ones. */
function countRelativePathSegments(path: string) {
  return path.split(/[\\/]/).filter((segment) => segment.length > 0).length
}

/** Uses git's ignore engine when available so discovery follows project ignore rules. */
async function isGitIgnoredPath(params: { gitRoot: string | null; path: string }) {
  if (!params.gitRoot) {
    return false
  }

  const checkedPath = await realpath(params.path).catch(() => params.path)
  const relativePath = relative(params.gitRoot, checkedPath)

  if (relativePath.length === 0 || relativePath === ".." || relativePath.startsWith("../")) {
    return false
  }

  const result = await runGitCommand(
    params.gitRoot,
    ["check-ignore", "-q", "--", `${relativePath}/`],
    {
      stdin: "ignore",
    },
  )
  return result.status === 0
}

/** Finds the first configured manifest present in one candidate subpackage directory. */
async function findSubpackageManifestPath(directory: string, manifests: readonly string[]) {
  for (const manifest of manifests) {
    const manifestPath = join(directory, manifest)

    try {
      if ((await stat(manifestPath)).isFile()) {
        return manifestPath
      }
    } catch {
      // Missing or unreadable manifests do not make the directory a package boundary.
    }
  }

  return null
}

/** Discovers selectable subpackage working directories under one project root. */
export async function discoverSessionSubpackages(params: {
  cwd: string
  configuredManifests?: readonly string[]
}) {
  const projectRoot = resolve(params.cwd)
  const manifests = resolveSubpackageManifestEntries(params.configuredManifests)
  const gitRoot = await resolveGitRepoRoot(projectRoot)
  const queue = [projectRoot]
  const subpackages: SessionSubpackage[] = []

  for (let index = 0; index < queue.length; index += 1) {
    const directory = queue[index]
    let entries: Dirent<string>[]

    try {
      entries = sortDirectoryEntries(await readDirectoryEntries(directory))
    } catch {
      continue
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || isIgnoredSubpackageDirectoryName(entry.name)) {
        continue
      }

      const directoryPath = join(directory, entry.name)

      if (await isGitIgnoredPath({ gitRoot, path: directoryPath })) {
        continue
      }

      const manifestPath = await findSubpackageManifestPath(directoryPath, manifests)

      if (manifestPath) {
        subpackages.push({
          path: directoryPath,
          relativePath: relative(projectRoot, directoryPath),
          name: basename(directoryPath),
          manifestPath,
        })
      }

      queue.push(directoryPath)
    }
  }

  subpackages.sort((left, right) => {
    const depthDiff =
      countRelativePathSegments(left.relativePath) - countRelativePathSegments(right.relativePath)

    if (depthDiff !== 0) {
      return depthDiff
    }

    return left.relativePath.localeCompare(right.relativePath)
  })

  return subpackages
}
