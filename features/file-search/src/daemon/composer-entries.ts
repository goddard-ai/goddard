import { type Dirent } from "node:fs"
import { readdir } from "node:fs/promises"
import { basename, join, relative, resolve } from "node:path"
import { pathToFileURL } from "node:url"
import type { FileFinder, MixedItem, Result } from "@ff-labs/fff-bun"

import type {
  FileSearchComposerEntriesRequest,
  FileSearchComposerEntriesResponse,
  FileSearchComposerEntry,
} from "../schema.ts"

const DEFAULT_COMPOSER_ENTRY_LIMIT = 20
const MAX_COMPOSER_ENTRY_LIMIT = 50
const COMPOSER_IGNORED_DIRECTORY_NAMES = new Set([
  ".cache",
  ".git",
  ".next",
  ".turbo",
  "build",
  "coverage",
  "dist",
  "node_modules",
  "out",
])
const FINDER_SCAN_WAIT_MS = 250
const FINDER_IDLE_MS = 10 * 60 * 1000
const MAX_CACHED_FINDERS = 8

type FileFinderInstance = Pick<FileFinder, "destroy" | "mixedSearch" | "waitForScan">

type FileFinderFactory = (basePath: string) => Promise<Result<FileFinderInstance>>

type FileFinderCacheEntry = {
  finder: FileFinderInstance
  lastUsedAt: number
  scanReady: Promise<void>
}

function normalizeLimit(limit: number | undefined) {
  return Math.min(Math.max(limit ?? DEFAULT_COMPOSER_ENTRY_LIMIT, 1), MAX_COMPOSER_ENTRY_LIMIT)
}

function formatDisplayPath(path: string) {
  return path.replaceAll("\\", "/")
}

function formatCwdRelativePath(cwd: string, path: string) {
  const relativePath = relative(cwd, path)

  if (relativePath.length === 0) {
    return "."
  }

  const displayPath = formatDisplayPath(relativePath)
  return displayPath.startsWith("..") ? displayPath : `./${displayPath}`
}

function sortDirectoryEntries(entries: readonly Dirent<string>[]) {
  return [...entries].sort((left, right) => {
    if (left.isDirectory() !== right.isDirectory()) {
      return left.isDirectory() ? -1 : 1
    }

    return left.name.localeCompare(right.name)
  })
}

function toComposerEntry(
  cwd: string,
  path: string,
  type: FileSearchComposerEntry["type"],
): FileSearchComposerEntry {
  return {
    type,
    path,
    uri: pathToFileURL(path).toString(),
    label: basename(path),
    detail: formatCwdRelativePath(cwd, path),
  }
}

function matchesQuery(cwd: string, path: string, query: string) {
  const normalizedQuery = query.trim().toLowerCase()

  if (normalizedQuery.length === 0) {
    return true
  }

  return (
    basename(path).toLowerCase().includes(normalizedQuery) ||
    formatCwdRelativePath(cwd, path).toLowerCase().includes(normalizedQuery)
  )
}

async function readDirectoryEntries(path: string) {
  return (await readdir(path, {
    encoding: "utf8",
    withFileTypes: true,
  })) as Dirent<string>[]
}

async function listImmediateEntries(cwd: string, limit: number) {
  const entries = sortDirectoryEntries(await readDirectoryEntries(cwd))
  const results: FileSearchComposerEntry[] = []

  for (const entry of entries) {
    if (entry.isDirectory() && COMPOSER_IGNORED_DIRECTORY_NAMES.has(entry.name)) {
      continue
    }

    if (!entry.isDirectory() && !entry.isFile()) {
      continue
    }

    results.push(
      toComposerEntry(cwd, join(cwd, entry.name), entry.isDirectory() ? "folder" : "file"),
    )

    if (results.length >= limit) {
      break
    }
  }

  return results
}

async function searchEntriesUnderCwd(cwd: string, query: string, limit: number) {
  const results: FileSearchComposerEntry[] = []

  async function visit(directory: string) {
    if (results.length >= limit) {
      return
    }

    let entries: Dirent<string>[]
    try {
      entries = sortDirectoryEntries(await readDirectoryEntries(directory))
    } catch {
      return
    }

    for (const entry of entries) {
      if (entry.isDirectory() && COMPOSER_IGNORED_DIRECTORY_NAMES.has(entry.name)) {
        continue
      }

      if (!entry.isDirectory() && !entry.isFile()) {
        continue
      }

      const path = join(directory, entry.name)
      const type = entry.isDirectory() ? "folder" : "file"

      if (matchesQuery(cwd, path, query)) {
        results.push(toComposerEntry(cwd, path, type))

        if (results.length >= limit) {
          return
        }
      }

      if (entry.isDirectory()) {
        await visit(path)

        if (results.length >= limit) {
          return
        }
      }
    }
  }

  await visit(cwd)
  return results
}

async function getFallbackComposerEntries(
  request: FileSearchComposerEntriesRequest,
  cwd: string,
  limit: number,
): Promise<FileSearchComposerEntriesResponse> {
  return {
    entries:
      request.query.trim().length === 0
        ? await listImmediateEntries(cwd, limit)
        : await searchEntriesUnderCwd(cwd, request.query, limit),
  }
}

async function createFffFinder(basePath: string): Promise<Result<FileFinder>> {
  const { FileFinder } = await import("@ff-labs/fff-bun")

  return FileFinder.create({
    basePath,
    aiMode: true,
    disableContentIndexing: true,
  })
}

function toFffComposerEntry(cwd: string, item: MixedItem): FileSearchComposerEntry {
  const relativePath = item.item.relativePath.replace(/[\\/]$/, "")
  const path = resolve(cwd, relativePath)

  return toComposerEntry(cwd, path, item.type === "directory" ? "folder" : "file")
}

export function createFileSearchManager(
  options: {
    createFinder?: FileFinderFactory
    now?: () => number
  } = {},
) {
  const createFinder = options.createFinder ?? createFffFinder
  const now = options.now ?? Date.now
  const finders = new Map<string, FileFinderCacheEntry>()

  function pruneFinders(currentTime = now()) {
    for (const [basePath, entry] of finders) {
      if (currentTime - entry.lastUsedAt <= FINDER_IDLE_MS) {
        continue
      }

      entry.finder.destroy()
      finders.delete(basePath)
    }

    const entriesByAge = [...finders.entries()].sort(
      (left, right) => left[1].lastUsedAt - right[1].lastUsedAt,
    )

    while (entriesByAge.length > MAX_CACHED_FINDERS) {
      const oldest = entriesByAge.shift()

      if (!oldest) {
        break
      }

      oldest[1].finder.destroy()
      finders.delete(oldest[0])
    }
  }

  async function getFinder(cwd: string) {
    const currentTime = now()
    pruneFinders(currentTime)

    const existing = finders.get(cwd)
    if (existing) {
      existing.lastUsedAt = currentTime
      return existing
    }

    const created = await createFinder(cwd)
    if (!created.ok) {
      return null
    }

    const entry: FileFinderCacheEntry = {
      finder: created.value,
      lastUsedAt: currentTime,
      scanReady: Promise.resolve().then(() => {
        const result = created.value.waitForScan(FINDER_SCAN_WAIT_MS)

        if (!result.ok) {
          throw new Error(result.error)
        }
      }),
    }

    finders.set(cwd, entry)
    pruneFinders(currentTime)
    return entry
  }

  async function composerEntries(
    request: FileSearchComposerEntriesRequest,
  ): Promise<FileSearchComposerEntriesResponse> {
    const cwd = resolve(request.cwd)
    const limit = normalizeLimit(request.limit)

    try {
      const entry = await getFinder(cwd)
      if (!entry) {
        return getFallbackComposerEntries(request, cwd, limit)
      }

      await entry.scanReady.catch(() => undefined)
      entry.lastUsedAt = now()

      const result = entry.finder.mixedSearch(request.query, {
        pageSize: limit,
      })

      if (!result.ok) {
        return getFallbackComposerEntries(request, cwd, limit)
      }

      return {
        entries: result.value.items.slice(0, limit).map((item) => toFffComposerEntry(cwd, item)),
      }
    } catch {
      return getFallbackComposerEntries(request, cwd, limit)
    }
  }

  function destroy() {
    for (const entry of finders.values()) {
      entry.finder.destroy()
    }

    finders.clear()
  }

  return {
    composerEntries,
    destroy,
  }
}

const fileSearchManager = createFileSearchManager()

export async function getComposerEntries(request: FileSearchComposerEntriesRequest) {
  return fileSearchManager.composerEntries(request)
}
