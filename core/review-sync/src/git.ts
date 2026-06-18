/** Git and filesystem helpers used by review-sync internals. */
import { spawn } from "node:child_process"
import { constants as fsConstants } from "node:fs"
import { access, realpath } from "node:fs/promises"
import { basename, isAbsolute, join, relative, resolve } from "node:path"

import { UserError } from "./errors.ts"
import type { RuntimeContext } from "./types.ts"

/** Parsed Git worktree-list entry used for branch ownership validation. */
export type WorktreeInfo = {
  path: string
  branch: string | null
}

/** Captured subprocess result used by Git command wrappers. */
export type CommandResult = {
  status: number
  stdout: string
  stderr: string
}

/** Options for running a raw Git command through the configured host. */
export type GitRunOptions = {
  allowFailure?: boolean
  stdin?: string | "ignore"
  env?: Record<string, string | undefined>
}

/** Internal Git access boundary used by review-sync operations. */
export type GitHost = {
  run: (cwd: string, args: string[], options?: GitRunOptions) => Promise<CommandResult>
  resolveRequiredRepoRoot: (cwd: string) => Promise<string>
  resolveRequiredGitCommonDir: (cwd: string) => Promise<string>
  resolveRequiredGitDir: (cwd: string) => Promise<string>
  resolveCurrentBranch: (cwd: string) => Promise<string | null>
  branchExists: (cwd: string, branch: string) => Promise<boolean>
  isWorktreeClean: (cwd: string) => Promise<boolean>
  resolveRef: (cwd: string, refName: string) => Promise<string | null>
  updateRef: (cwd: string, refName: string, oid: string) => Promise<void>
  deleteRef: (cwd: string, refName: string) => Promise<void>
  listWorktrees: (cwd: string) => Promise<WorktreeInfo[]>
}

type BunFfi = typeof import("bun:ffi")
type Libgit2Symbols = ReturnType<typeof loadLibgit2>["symbols"]
type FfiPointer = Parameters<Libgit2Symbols["git_repository_free"]>[0]

const bunFfi = await loadBunFfiForSelectedHost()

/** Creates the Git host selected for this process. */
export function createReviewSyncGitHost() {
  if (process.env.REVIEW_SYNC_GIT_HOST === "libgit2") {
    return createLibgit2GitHost(createCliGitHost())
  }

  return createCliGitHost()
}

/** Creates the default Git host backed by Git CLI subprocesses. */
export function createCliGitHost() {
  const run = async (cwd: string, args: string[], options: GitRunOptions = {}) => {
    const result = await runCommand("git", args, {
      cwd,
      stdin: options.stdin,
      env: {
        ...process.env,
        ...options.env,
      },
    })

    if (result.status !== 0 && options.allowFailure !== true) {
      throw new Error(
        `git ${args.join(" ")} failed in ${cwd}: ${
          result.stderr.trim() || result.stdout.trim() || "unknown Git error"
        }`,
      )
    }

    return result
  }

  return {
    run,
    resolveRequiredRepoRoot: async (cwd) => {
      const result = await run(cwd, ["rev-parse", "--show-toplevel"], {
        allowFailure: true,
      })
      if (result.status !== 0 || !result.stdout.trim()) {
        throw new UserError(`Not a Git worktree: ${cwd}`)
      }
      return await normalizePath(result.stdout.trim())
    },
    resolveRequiredGitCommonDir: async (cwd) => {
      const result = await run(cwd, ["rev-parse", "--git-common-dir"])
      const value = result.stdout.trim()
      return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
    },
    resolveRequiredGitDir: async (cwd) => {
      const result = await run(cwd, ["rev-parse", "--git-dir"])
      const value = result.stdout.trim()
      return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
    },
    resolveCurrentBranch: async (cwd) => {
      const result = await run(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], {
        allowFailure: true,
      })
      return result.status === 0 ? result.stdout.trim() || null : null
    },
    branchExists: async (cwd, branch) => {
      const result = await run(cwd, ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`], {
        allowFailure: true,
      })
      return result.status === 0
    },
    isWorktreeClean: async (cwd) => {
      const result = await run(cwd, ["status", "--porcelain=v1", "--untracked-files=all"])
      return !result.stdout.trim()
    },
    resolveRef: async (cwd, refName) => {
      const result = await run(cwd, ["rev-parse", "--verify", "-q", refName], {
        allowFailure: true,
      })
      return result.status === 0 ? result.stdout.trim() || null : null
    },
    updateRef: async (cwd, refName, oid) => {
      await run(cwd, ["update-ref", refName, oid])
    },
    deleteRef: async (cwd, refName) => {
      await run(cwd, ["update-ref", "-d", refName], {
        allowFailure: true,
      })
    },
    listWorktrees: async (cwd) => {
      const result = await run(cwd, ["worktree", "list", "--porcelain"])
      return await parseGitWorktrees(result.stdout)
    },
  } satisfies GitHost
}

/** Creates an experimental Git host that uses libgit2 for read-heavy lookups. */
export function createLibgit2GitHost(fallback: GitHost) {
  if (!bunFfi) {
    throw new Error("The libgit2 Git host requires Bun.")
  }

  const libgit2 = loadLibgit2(bunFfi)
  const initStatus = libgit2.symbols.git_libgit2_init()
  if (initStatus < 0) {
    throw new Error(`git_libgit2_init failed with status ${initStatus}`)
  }

  return {
    ...fallback,
    resolveRequiredRepoRoot: async (cwd) =>
      withLibgit2Repository(libgit2.symbols, cwd, async (repo) => {
        const workdir = libgit2.symbols.git_repository_workdir(repo)
        if (!workdir) {
          throw new UserError(`Not a Git worktree: ${cwd}`)
        }
        return await normalizePath(String(workdir))
      }),
    resolveRequiredGitCommonDir: async (cwd) =>
      withLibgit2Repository(libgit2.symbols, cwd, async (repo) => {
        const commonDir = libgit2.symbols.git_repository_commondir(repo)
        if (!commonDir) {
          throw new Error(`libgit2 could not resolve Git common dir for ${cwd}`)
        }
        return await normalizePath(String(commonDir))
      }),
    resolveRequiredGitDir: async (cwd) =>
      withLibgit2Repository(libgit2.symbols, cwd, async (repo) => {
        const gitDir = libgit2.symbols.git_repository_path(repo)
        if (!gitDir) {
          throw new Error(`libgit2 could not resolve Git dir for ${cwd}`)
        }
        return await normalizePath(String(gitDir))
      }),
    resolveCurrentBranch: async (cwd) =>
      withLibgit2Repository(libgit2.symbols, cwd, (repo) => {
        const out = pointerStorage()
        const status = libgit2.symbols.git_repository_head(bunFfi.ptr(out), repo)
        if (status !== 0) {
          return null
        }

        const head = readPointer(out)
        try {
          const name = libgit2.symbols.git_reference_name(head)
          const branchRef = name ? String(name) : ""
          return branchRef.startsWith("refs/heads/") ? branchRef.slice("refs/heads/".length) : null
        } finally {
          libgit2.symbols.git_reference_free(head)
        }
      }),
    branchExists: async (cwd, branch) =>
      withLibgit2Repository(
        libgit2.symbols,
        cwd,
        (repo) =>
          libgit2.symbols.git_reference_name_to_id(
            bunFfi.ptr(new Uint8Array(20)),
            repo,
            bunFfi.ptr(cString(`refs/heads/${branch}`)),
          ) === 0,
      ),
    resolveRef: async (cwd, refName) =>
      withLibgit2Repository(libgit2.symbols, cwd, (repo) => {
        const out = pointerStorage()
        const status = libgit2.symbols.git_revparse_single(
          bunFfi.ptr(out),
          repo,
          bunFfi.ptr(cString(refName)),
        )
        if (status !== 0) {
          return null
        }

        const object = readPointer(out)
        try {
          const oid = libgit2.symbols.git_object_id(object)
          if (!oid) {
            return null
          }
          return String(libgit2.symbols.git_oid_tostr_s(oid))
        } finally {
          libgit2.symbols.git_object_free(object)
        }
      }),
  } satisfies GitHost
}

async function loadBunFfiForSelectedHost() {
  if (process.env.REVIEW_SYNC_GIT_HOST !== "libgit2") {
    return null
  }

  if (!("bun" in process.versions)) {
    throw new Error("REVIEW_SYNC_GIT_HOST=libgit2 requires Bun.")
  }

  return await import("bun:ffi")
}

function loadLibgit2(ffi: BunFfi) {
  const errors: string[] = []
  const libgit2PathCandidates = [
    process.env.LIBGIT2_PATH,
    `libgit2.${ffi.suffix}`,
    `/opt/homebrew/lib/libgit2.${ffi.suffix}`,
    `/usr/local/lib/libgit2.${ffi.suffix}`,
  ].filter((path) => typeof path === "string")

  for (const candidate of libgit2PathCandidates) {
    try {
      return ffi.dlopen(candidate, {
        git_libgit2_init: {
          args: [],
          returns: ffi.FFIType.i32,
        },
        git_repository_open_ext: {
          args: [ffi.FFIType.ptr, ffi.FFIType.cstring, ffi.FFIType.u32, ffi.FFIType.ptr],
          returns: ffi.FFIType.i32,
        },
        git_repository_free: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.void,
        },
        git_repository_path: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.cstring,
        },
        git_repository_workdir: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.cstring,
        },
        git_repository_commondir: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.cstring,
        },
        git_repository_head: {
          args: [ffi.FFIType.ptr, ffi.FFIType.ptr],
          returns: ffi.FFIType.i32,
        },
        git_reference_name: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.cstring,
        },
        git_reference_name_to_id: {
          args: [ffi.FFIType.ptr, ffi.FFIType.ptr, ffi.FFIType.cstring],
          returns: ffi.FFIType.i32,
        },
        git_reference_free: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.void,
        },
        git_revparse_single: {
          args: [ffi.FFIType.ptr, ffi.FFIType.ptr, ffi.FFIType.cstring],
          returns: ffi.FFIType.i32,
        },
        git_object_id: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.ptr,
        },
        git_object_free: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.void,
        },
        git_oid_tostr_s: {
          args: [ffi.FFIType.ptr],
          returns: ffi.FFIType.cstring,
        },
      })
    } catch (error) {
      errors.push(`${candidate}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  throw new Error(`Unable to load libgit2. Tried: ${errors.join("; ")}`)
}

function pointerStorage() {
  return new BigUint64Array(1)
}

function readPointer(storage: BigUint64Array) {
  return Number(storage[0]) as FfiPointer
}

function cString(value: string) {
  return Buffer.from(`${value}\0`)
}

async function withLibgit2Repository<T>(
  libgit2: Libgit2Symbols,
  cwd: string,
  operation: (repo: FfiPointer) => T | Promise<T>,
) {
  if (!bunFfi) {
    throw new Error("The libgit2 Git host requires Bun.")
  }

  const out = pointerStorage()
  const status = libgit2.git_repository_open_ext(bunFfi.ptr(out), bunFfi.ptr(cString(cwd)), 0, 0)
  if (status !== 0) {
    throw new UserError(`Not a Git worktree: ${cwd}`)
  }

  const repo = readPointer(out)
  try {
    return await operation(repo)
  } finally {
    libgit2.git_repository_free(repo)
  }
}

/** Runs one Git command and returns captured stdout/stderr. */
export async function git(
  cwd: string,
  args: string[],
  context: RuntimeContext,
  options: GitRunOptions = {},
) {
  return await context.gitHost.run(cwd, args, options)
}

/** Resolves the repository root for a path or raises a user-facing error. */
export async function resolveRequiredRepoRoot(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredRepoRoot(cwd)
}

/** Resolves one worktree's Git common directory as an absolute path. */
export async function resolveRequiredGitCommonDir(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredGitCommonDir(cwd)
}

/** Resolves one worktree's per-worktree Git metadata directory as an absolute path. */
export async function resolveRequiredGitDir(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredGitDir(cwd)
}

/** Returns the attached branch name, or null for detached HEAD. */
export async function resolveCurrentBranch(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveCurrentBranch(cwd)
}

/** Checks whether a local branch already exists. */
export async function branchExists(cwd: string, branch: string, context: RuntimeContext) {
  return await context.gitHost.branchExists(cwd, branch)
}

/** Checks whether a worktree has tracked, unstaged, staged, or untracked non-ignored changes. */
export async function isWorktreeClean(cwd: string, context: RuntimeContext) {
  return await context.gitHost.isWorktreeClean(cwd)
}

/** Reads one ref and returns null when it does not exist. */
export async function resolveRef(cwd: string, refName: string, context: RuntimeContext) {
  return await context.gitHost.resolveRef(cwd, refName)
}

/** Updates or creates one hidden ref. */
export async function updateRef(
  cwd: string,
  refName: string,
  oid: string,
  context: RuntimeContext,
) {
  await context.gitHost.updateRef(cwd, refName, oid)
}

/** Deletes one hidden ref, allowing retry after a previous cleanup removed it. */
export async function deleteRef(cwd: string, refName: string, context: RuntimeContext) {
  await context.gitHost.deleteRef(cwd, refName)
}

/** Ensures a review branch is not checked out outside the configured review worktree. */
export async function assertReviewBranchNotCheckedOutElsewhere(input: {
  cwd: string
  reviewBranch: string
  reviewWorktree: string
  context: RuntimeContext
}) {
  const worktrees = await listGitWorktrees(input.cwd, input.context)
  for (const worktree of worktrees) {
    if (worktree.branch !== input.reviewBranch) {
      continue
    }
    if (worktree.path !== input.reviewWorktree) {
      throw new UserError(
        `Review branch ${input.reviewBranch} is already checked out at ${worktree.path}.`,
      )
    }
  }
}

/** Refuses sync while Git has an unresolved operation in progress. */
export async function assertSupportedGitState(cwd: string, context: RuntimeContext) {
  const gitDir = await resolveRequiredGitDir(cwd, context)
  const commonDir = await resolveRequiredGitCommonDir(cwd, context)
  const markers = [
    join(gitDir, "MERGE_HEAD"),
    join(gitDir, "CHERRY_PICK_HEAD"),
    join(gitDir, "REVERT_HEAD"),
    join(gitDir, "rebase-merge"),
    join(gitDir, "rebase-apply"),
    join(gitDir, "BISECT_LOG"),
    join(commonDir, "BISECT_LOG"),
  ]

  for (const marker of markers) {
    if (await pathExists(marker)) {
      throw new UserError(`Unsupported in-progress Git state in ${cwd}: ${basename(marker)}.`)
    }
  }

  const rebaseHead = join(gitDir, "REBASE_HEAD")
  if ((await pathExists(rebaseHead)) && !(await isStaleRebaseHead(cwd, context))) {
    throw new UserError(`Unsupported in-progress Git state in ${cwd}: ${basename(rebaseHead)}.`)
  }
}

/** Distinguishes abandoned REBASE_HEAD files from a Git-recognized active rebase. */
async function isStaleRebaseHead(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rebase", "--show-current-patch"], context, {
    allowFailure: true,
  })
  if (result.status === 0) {
    return false
  }

  return `${result.stderr}\n${result.stdout}`.toLowerCase().includes("no rebase in progress")
}

/** Resolves and canonicalizes an existing filesystem path. */
export async function normalizePath(path: string) {
  return await realpath(resolve(path))
}

/** Tests whether child is equal to or nested under parent. */
export function isInsideOrEqual(parent: string, child: string) {
  const rel = relative(parent, child)
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel))
}

/** Checks whether one filesystem path currently exists. */
export async function pathExists(path: string) {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Checks whether a thrown value is a Node filesystem error with the requested code. */
export function isNodeErrorWithCode(error: unknown, code: string) {
  return (
    error instanceof Error &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code === code
  )
}

/** Checks whether a local process id still exists. */
export function isProcessAlive(pid: number) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/** Parses Git's porcelain worktree list into path and branch records. */
export async function listGitWorktrees(cwd: string, context: RuntimeContext) {
  return await context.gitHost.listWorktrees(cwd)
}

async function parseGitWorktrees(stdout: string) {
  const worktrees: WorktreeInfo[] = []
  let current: Partial<WorktreeInfo> = {}

  for (const line of stdout.split("\n")) {
    if (!line.trim()) {
      if (current.path) {
        worktrees.push({
          path: await normalizePath(current.path),
          branch: current.branch ?? null,
        })
      }
      current = {}
      continue
    }

    if (line.startsWith("worktree ")) {
      current.path = line.slice("worktree ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    }
  }

  if (current.path) {
    worktrees.push({
      path: await normalizePath(current.path),
      branch: current.branch ?? null,
    })
  }

  return worktrees
}

/** Runs a subprocess with captured output and optional stdin. */
async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd: string
    stdin?: string | "ignore"
    env: Record<string, string | undefined>
  },
) {
  return await new Promise<CommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
    })

    let stdout = ""
    let stderr = ""
    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })

    if (options.stdin && options.stdin !== "ignore") {
      child.stdin.end(options.stdin)
    } else {
      child.stdin.end()
    }
  })
}
