import { constants as fsConstants, type Dirent } from "node:fs"
import { access, readdir } from "node:fs/promises"
import { homedir } from "node:os"
import { basename, dirname, join, relative, resolve } from "node:path"
import { pathToFileURL } from "node:url"
import * as acp from "@agentclientprotocol/sdk"
import type {
  SessionComposerFileSuggestion,
  SessionComposerSkillSuggestion,
  SessionComposerSlashCommandSuggestion,
  SessionComposerSuggestionsResponse,
  SessionDraftSuggestionsRequest,
} from "@goddard-ai/schema/daemon"

const DEFAULT_COMPOSER_SUGGESTION_LIMIT = 20
export const MAX_COMPOSER_SUGGESTION_LIMIT = 50
const COMPOSER_IGNORED_DIRECTORY_NAMES = new Set([".git", "node_modules", "dist"])

/** Returns true when one filesystem path currently exists. */
async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Reads one directory with string entry names across Node and Bun. */
async function readDirectoryEntries(path: string) {
  return (await readdir(path, {
    encoding: "utf-8",
    withFileTypes: true,
  })) as Dirent<string>[]
}

/** Returns true when one unknown value is a plain object record. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

/** Bounds the chat-composer suggestion limit to one small stable range. */
export function normalizeComposerSuggestionLimit(limit: number | undefined) {
  return Math.min(
    Math.max(limit ?? DEFAULT_COMPOSER_SUGGESTION_LIMIT, 1),
    MAX_COMPOSER_SUGGESTION_LIMIT,
  )
}

/** Resolves the current user home directory while respecting test overrides. */
function getUserHomeDir() {
  return process.env.HOME || homedir()
}

/** Formats one path relative to the active session cwd for compact UI display. */
function formatCwdRelativePath(cwd: string, path: string) {
  const relativePath = relative(cwd, path)

  if (relativePath.length === 0) {
    return "."
  }

  return relativePath.startsWith("..") ? relativePath : `./${relativePath}`
}

/** Formats one path relative to the user home directory when possible. */
function formatHomeRelativePath(path: string) {
  const relativePath = relative(getUserHomeDir(), path)

  if (relativePath.length === 0) {
    return "~"
  }

  return relativePath.startsWith("..") ? path : `~/${relativePath}`
}

/** Converts one filesystem path into the ACP-friendly file URI used for resource links. */
function toFileUri(path: string) {
  return pathToFileURL(path).toString()
}

/** Produces one stable display suggestion for a file or folder under the session cwd. */
function toFilesystemSuggestion(cwd: string, path: string, type: "file" | "folder") {
  return {
    type,
    path,
    uri: toFileUri(path),
    label: basename(path),
    detail: formatCwdRelativePath(cwd, path),
  } satisfies SessionComposerFileSuggestion
}

/** Returns true when one filesystem entry matches the current case-insensitive query. */
function matchesFilesystemQuery(cwd: string, path: string, query: string) {
  const normalizedQuery = query.trim().toLowerCase()

  if (normalizedQuery.length === 0) {
    return true
  }

  return (
    basename(path).toLowerCase().includes(normalizedQuery) ||
    formatCwdRelativePath(cwd, path).toLowerCase().includes(normalizedQuery)
  )
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

/** Reads immediate child suggestions for one empty `@` lookup. */
async function listComposerEntriesAtCwd(cwd: string, limit: number) {
  const entries = sortDirectoryEntries(await readDirectoryEntries(cwd))
  const suggestions: SessionComposerFileSuggestion[] = []

  for (const entry of entries) {
    if (entry.isDirectory() && COMPOSER_IGNORED_DIRECTORY_NAMES.has(entry.name)) {
      continue
    }

    if (!entry.isDirectory() && !entry.isFile()) {
      continue
    }

    const path = join(cwd, entry.name)
    suggestions.push(toFilesystemSuggestion(cwd, path, entry.isDirectory() ? "folder" : "file"))

    if (suggestions.length >= limit) {
      break
    }
  }

  return suggestions
}

/** Recursively searches the session cwd subtree for matching file and folder suggestions. */
async function searchComposerEntriesUnderCwd(cwd: string, query: string, limit: number) {
  const suggestions: SessionComposerFileSuggestion[] = []

  async function visit(directory: string) {
    if (suggestions.length >= limit) {
      return
    }

    const entries = sortDirectoryEntries(await readDirectoryEntries(directory))

    for (const entry of entries) {
      if (entry.isDirectory() && COMPOSER_IGNORED_DIRECTORY_NAMES.has(entry.name)) {
        continue
      }

      if (!entry.isDirectory() && !entry.isFile()) {
        continue
      }

      const path = join(directory, entry.name)
      const type = entry.isDirectory() ? "folder" : "file"

      if (matchesFilesystemQuery(cwd, path, query)) {
        suggestions.push(toFilesystemSuggestion(cwd, path, type))

        if (suggestions.length >= limit) {
          return
        }
      }

      if (entry.isDirectory()) {
        await visit(path)

        if (suggestions.length >= limit) {
          return
        }
      }
    }
  }

  await visit(cwd)
  return suggestions
}

/** Resolves the nearest `.agents/skills` directory reachable from the session cwd. */
async function findNearestSkillRoot(cwd: string) {
  let current = resolve(cwd)

  while (true) {
    const candidate = join(current, ".agents", "skills")

    if (await pathExists(candidate)) {
      return candidate
    }

    const parent = dirname(current)

    if (parent === current) {
      return null
    }

    current = parent
  }
}

/** Reads one skill root into stable `$` composer suggestion items. */
async function readSkillSuggestions(params: {
  cwd: string
  root: string
  source: "local" | "global"
  query: string
}) {
  if (!(await pathExists(params.root))) {
    return [] satisfies SessionComposerSuggestionsResponse["suggestions"]
  }

  const entries = sortDirectoryEntries(await readDirectoryEntries(params.root))
  const normalizedQuery = params.query.trim().toLowerCase()
  const suggestions: SessionComposerSkillSuggestion[] = []

  for (const entry of entries) {
    if (!entry.isDirectory()) {
      continue
    }

    const skillPath = join(params.root, entry.name, "SKILL.md")

    if (!(await pathExists(skillPath))) {
      continue
    }

    const detail =
      params.source === "global"
        ? formatHomeRelativePath(skillPath)
        : formatCwdRelativePath(params.cwd, skillPath)

    if (
      normalizedQuery.length > 0 &&
      entry.name.toLowerCase().includes(normalizedQuery) === false &&
      detail.toLowerCase().includes(normalizedQuery) === false
    ) {
      continue
    }

    suggestions.push({
      type: "skill",
      path: skillPath,
      uri: toFileUri(skillPath),
      label: entry.name,
      detail,
      source: params.source,
    })
  }

  return suggestions
}

/** Merges local and global skill roots while preserving local name precedence. */
async function getSkillComposerSuggestions(cwd: string, query: string, limit: number) {
  const localRoot = await findNearestSkillRoot(cwd)
  const globalRoot = join(getUserHomeDir(), ".agents", "skills")
  const [localSuggestions, globalSuggestions] = await Promise.all([
    localRoot ? readSkillSuggestions({ cwd, root: localRoot, source: "local", query }) : [],
    readSkillSuggestions({ cwd, root: globalRoot, source: "global", query }),
  ])
  const suggestions: SessionComposerSkillSuggestion[] = []
  const seenLabels = new Set<string>()

  for (const suggestion of [...localSuggestions, ...globalSuggestions]) {
    if (seenLabels.has(suggestion.label)) {
      continue
    }

    seenLabels.add(suggestion.label)
    suggestions.push(suggestion)

    if (suggestions.length >= limit) {
      break
    }
  }

  return suggestions
}

/** Filters the latest ACP slash commands into session composer suggestion items. */
export function getSlashComposerSuggestions(
  availableCommands: readonly acp.AvailableCommand[],
  query: string,
  limit: number,
) {
  const normalizedQuery = query.trim().toLowerCase()
  const suggestions: SessionComposerSlashCommandSuggestion[] = []

  for (const command of availableCommands) {
    const inputHint =
      isRecord(command.input) && typeof command.input.hint === "string" ? command.input.hint : null

    if (
      normalizedQuery.length > 0 &&
      command.name.toLowerCase().includes(normalizedQuery) === false &&
      command.description.toLowerCase().includes(normalizedQuery) === false &&
      (inputHint?.toLowerCase().includes(normalizedQuery) ?? false) === false
    ) {
      continue
    }

    suggestions.push({
      type: "slash_command",
      name: command.name,
      description: command.description,
      inputHint,
    })

    if (suggestions.length >= limit) {
      break
    }
  }

  return suggestions
}

/** Resolves the current set of draft composer suggestions before a daemon session exists. */
export async function getDraftComposerSuggestions(params: {
  cwd: string
  trigger: SessionDraftSuggestionsRequest["trigger"]
  query: string
  limit: number
}) {
  if (params.trigger === "at") {
    return params.query.trim().length === 0
      ? await listComposerEntriesAtCwd(params.cwd, params.limit)
      : await searchComposerEntriesUnderCwd(params.cwd, params.query, params.limit)
  }

  return await getSkillComposerSuggestions(params.cwd, params.query, params.limit)
}
