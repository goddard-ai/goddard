import { randomInt } from "node:crypto"
import { userInfo } from "node:os"

const WORKTREE_BRANCH_ADJECTIVES = [
  "brave",
  "bright",
  "calm",
  "clear",
  "clever",
  "easy",
  "fresh",
  "gentle",
  "happy",
  "kind",
  "light",
  "lucky",
  "neat",
  "quick",
  "ready",
  "sharp",
  "smooth",
  "steady",
  "swift",
  "tidy",
]

const WORKTREE_BRANCH_NOUNS = [
  "book",
  "bridge",
  "button",
  "cloud",
  "desk",
  "field",
  "file",
  "garden",
  "glass",
  "key",
  "lamp",
  "map",
  "paper",
  "path",
  "river",
  "stone",
  "table",
  "tool",
  "trail",
  "window",
]

const WORKTREE_BRANCH_VERBS = [
  "build",
  "check",
  "create",
  "draw",
  "fetch",
  "find",
  "fix",
  "grow",
  "help",
  "join",
  "learn",
  "make",
  "move",
  "open",
  "read",
  "repair",
  "send",
  "shape",
  "test",
  "write",
]

const DEFAULT_WORKTREE_BRANCH_PREFIX = "goddard"
const DEFAULT_PULL_REQUEST_BRANCH_HOST = "github.com"
const WORKTREE_BRANCH_GENERATION_ATTEMPTS = 100

/** Resolves an unused generated branch name for a new daemon-managed session worktree. */
export async function resolveAvailableWorktreeBranchName(params: {
  cwd: string
  branchPrefix?: string
}) {
  for (let attempt = 0; attempt < WORKTREE_BRANCH_GENERATION_ATTEMPTS; attempt += 1) {
    const branchName = resolveWorktreeBranchName({
      readableId: createWorktreeBranchReadableId(),
      branchPrefix: params.branchPrefix,
    })

    if (!(await gitBranchExists(params.cwd, branchName))) {
      return branchName
    }
  }

  throw new Error("Unable to generate an unused worktree branch name for this repository.")
}

/** Resolves the branch name used when creating a daemon-managed session worktree. */
export function resolveWorktreeBranchName(params: {
  readableId: string
  repository?: string
  prNumber?: number
  branchPrefix?: string
}) {
  if (typeof params.prNumber === "number") {
    return `${resolvePullRequestBranchHost(params.repository)}/pr/${params.prNumber}`
  }

  return `${resolveWorktreeBranchPrefix(params.branchPrefix)}/${sanitizeBranchPathComponent(params.readableId) || "worktree"}`
}

/** Creates a human-readable branch id from easy-to-type words instead of internal session ids. */
export function createWorktreeBranchReadableId() {
  return [
    randomReadableWord(WORKTREE_BRANCH_ADJECTIVES),
    randomReadableWord(WORKTREE_BRANCH_NOUNS),
    randomReadableWord(WORKTREE_BRANCH_VERBS),
  ].join("-")
}

/** Resolves the configured worktree branch prefix, defaulting to the local user name. */
export function resolveWorktreeBranchPrefix(configuredPrefix?: string) {
  const prefix =
    configuredPrefix ??
    readLocalUsername() ??
    process.env.USER ??
    process.env.USERNAME ??
    DEFAULT_WORKTREE_BRANCH_PREFIX

  return sanitizeBranchPath(prefix, DEFAULT_WORKTREE_BRANCH_PREFIX)
}

function randomReadableWord(words: readonly string[]) {
  return words[randomInt(words.length)]
}

function resolvePullRequestBranchHost(repository?: string) {
  return (
    sanitizeBranchPathComponent(
      readRepositoryHost(repository) ?? DEFAULT_PULL_REQUEST_BRANCH_HOST,
    ) || DEFAULT_PULL_REQUEST_BRANCH_HOST
  )
}

function readRepositoryHost(repository?: string) {
  const value = repository?.trim()
  if (!value) {
    return null
  }

  try {
    const url = new URL(value)
    return url.hostname || null
  } catch {}

  const sshMatch = /^git@([^:]+):/.exec(value)
  if (sshMatch) {
    return sshMatch[1]
  }

  const [firstSegment] = value.split("/")
  if (firstSegment?.includes(".")) {
    return firstSegment
  }

  return null
}

function readLocalUsername() {
  try {
    return userInfo().username || null
  } catch {
    return null
  }
}

async function gitBranchExists(cwd: string, branchName: string) {
  const result = Bun.spawn(["git", "show-ref", "--verify", "--quiet", `refs/heads/${branchName}`], {
    cwd,
    stdin: "ignore",
    stdout: "ignore",
    stderr: "ignore",
  })

  await result.exited
  return result.exitCode === 0
}

function sanitizeBranchPath(value: string, fallback: string) {
  const path = value
    .split("/")
    .map((segment) => sanitizeBranchPathComponent(segment))
    .filter((segment) => segment.length > 0)
    .join("/")

  return path || fallback
}

function sanitizeBranchPathComponent(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/\.+/g, ".")
    .replace(/^-+|-+$/g, "")
    .replace(/^\.+|\.+$/g, "")
    .replace(/\.lock$/g, "")
}
