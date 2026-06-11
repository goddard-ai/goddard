import { type Dirent } from "node:fs"
import { readdir } from "node:fs/promises"
import { basename, relative, resolve } from "node:path"
import { pathToFileURL } from "node:url"

import type {
  FileSearchComposerEntriesRequest,
  FileSearchComposerEntriesResponse,
  FileSearchComposerEntry,
} from "../schema.ts"

const DEFAULT_COMPOSER_ENTRY_LIMIT = 20
const MAX_COMPOSER_ENTRY_LIMIT = 50
const IGNORED_DIRECTORY_NAMES = new Set([".git", "node_modules", "dist"])

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

/** Reads deterministic local file/folder entries for the initial file-search contract. */
export async function getComposerEntries(
  request: FileSearchComposerEntriesRequest,
): Promise<FileSearchComposerEntriesResponse> {
  const cwd = resolve(request.cwd)
  const limit = normalizeLimit(request.limit)
  const entries = sortDirectoryEntries(
    await readdir(cwd, { encoding: "utf8", withFileTypes: true }),
  )
  const results: FileSearchComposerEntry[] = []

  for (const entry of entries) {
    if (entry.isDirectory() && IGNORED_DIRECTORY_NAMES.has(entry.name)) {
      continue
    }

    if (!entry.isDirectory() && !entry.isFile()) {
      continue
    }

    const path = resolve(cwd, entry.name)

    if (!matchesQuery(cwd, path, request.query)) {
      continue
    }

    results.push(toComposerEntry(cwd, path, entry.isDirectory() ? "folder" : "file"))

    if (results.length >= limit) {
      break
    }
  }

  return {
    entries: results,
  }
}
