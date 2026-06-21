/** Shared Git access boundary for daemon-owned repository behavior. */
import { spawn } from "node:child_process"
import { realpath } from "node:fs/promises"
import { isAbsolute, resolve } from "node:path"
import { dlopen, FFIType, ptr, suffix } from "bun:ffi"

export type WorktreeInfo = {
  path: string
  branch: string | null
}

export type WorkingTreeStatus = {
  clean: boolean
  entries: string[]
}

export type GitCommandResult = {
  status: number
  stdout: string
  stderr: string
}

export type GitRunOptions = {
  allowFailure?: boolean
  stdin?: string | "ignore"
  env?: Record<string, string | undefined>
}

export type GitHostMode = "auto" | "cli" | "libgit2"

export type GitHostOptions = {
  mode?: GitHostMode
  libgit2PathCandidates?: string[]
}

export type GitRepositoryApi = {
  resolveRoot: (cwd: string) => Promise<string>
  resolveGitDir: (cwd: string) => Promise<string>
  resolveCommonDir: (cwd: string) => Promise<string>
  resolveGitPath: (cwd: string, gitPath: string) => Promise<string>
  isBareRepository: (cwd: string) => Promise<boolean>
}

export type GitRefsApi = {
  resolve: (cwd: string, refName: string) => Promise<string | null>
  exists: (cwd: string, refName: string) => Promise<boolean>
  update: (cwd: string, refName: string, oid: string) => Promise<void>
  delete: (cwd: string, refName: string) => Promise<void>
  getCurrentBranch: (cwd: string) => Promise<string | null>
  branchExists: (cwd: string, branch: string) => Promise<boolean>
  getBranchHead: (cwd: string, branch: string) => Promise<string | null>
}

export type GitHistoryApi = {
  resolveHead: (cwd: string) => Promise<string | null>
  isAncestor: (cwd: string, ancestor: string, descendant: string) => Promise<boolean>
  getMergeBase: (cwd: string, left: string, right: string) => Promise<string | null>
}

export type GitStatusApi = {
  getWorkingTreeStatus: (cwd: string) => Promise<WorkingTreeStatus>
  isWorktreeClean: (cwd: string) => Promise<boolean>
}

export type GitWorktreeApi = {
  list: (cwd: string) => Promise<WorktreeInfo[]>
}

export type GitHost = {
  repository: GitRepositoryApi
  refs: GitRefsApi
  history: GitHistoryApi
  status: GitStatusApi
  worktrees: GitWorktreeApi
}

type Libgit2Symbols = ReturnType<typeof loadLibgit2>["symbols"]
type FfiPointer = Parameters<Libgit2Symbols["git_repository_free"]>[0]

let initializedLibgit2: Libgit2Symbols | undefined

export class GitHostError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = "GitHostError"
  }
}

export class GitNotRepositoryError extends GitHostError {
  constructor(cwd: string) {
    super(`Not a Git worktree: ${cwd}`)
    this.name = "GitNotRepositoryError"
  }
}

export class GitCommandError extends GitHostError {
  args: string[]
  cwd: string
  stdout: string
  stderr: string
  status: number

  constructor(cwd: string, args: string[], result: GitCommandResult) {
    super(
      `git ${args.join(" ")} failed in ${cwd}: ${
        result.stderr.trim() || result.stdout.trim() || "unknown Git error"
      }`,
    )
    this.name = "GitCommandError"
    this.cwd = cwd
    this.args = args
    this.stdout = result.stdout
    this.stderr = result.stderr
    this.status = result.status
  }
}

export function createGitHost(options: GitHostOptions = {}) {
  const mode = options.mode ?? resolveGitHostMode()
  const fallback = createCliGitHost()

  if (mode === "cli") {
    return fallback
  }

  try {
    return createLibgit2GitHost(fallback, {
      fallbackOnOperationError: mode === "auto",
      libgit2PathCandidates: options.libgit2PathCandidates,
    })
  } catch (error) {
    if (mode === "auto") {
      return fallback
    }
    throw error
  }
}

export function resolveGitHostMode(env: Record<string, string | undefined> = process.env) {
  if (env.GODDARD_GIT_HOST === "cli") {
    return "cli"
  }
  if (env.GODDARD_GIT_HOST === "libgit2") {
    return "libgit2"
  }
  return "auto"
}

export function resetGitHostForTests() {
  initializedLibgit2 = undefined
}

export function createCliGitHost(): GitHost {
  const run = async (cwd: string, args: string[], options: GitRunOptions = {}) => {
    const result = await runGitCommand(cwd, args, options)
    if (result.status !== 0 && options.allowFailure !== true) {
      throw new GitCommandError(cwd, args, result)
    }

    return result
  }

  return {
    repository: {
      resolveRoot: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--show-toplevel"], {
          allowFailure: true,
        })
        if (result.status !== 0 || !result.stdout.trim()) {
          throw new GitNotRepositoryError(cwd)
        }
        return await normalizePath(result.stdout.trim())
      },
      resolveGitDir: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--git-dir"])
        return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
      },
      resolveCommonDir: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--git-common-dir"])
        return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
      },
      resolveGitPath: async (cwd, gitPath) => {
        const result = await run(cwd, ["rev-parse", "--git-path", gitPath])
        return resolveGitOutputPath(cwd, result.stdout.trim())
      },
      isBareRepository: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--is-bare-repository"], {
          allowFailure: true,
        })
        return result.status === 0 && result.stdout.trim() === "true"
      },
    },
    refs: {
      resolve: async (cwd, refName) => {
        const result = await run(cwd, ["rev-parse", "--verify", "-q", refName], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
      exists: async (cwd, refName) => {
        const result = await run(cwd, ["rev-parse", "--verify", "--quiet", refName], {
          allowFailure: true,
        })
        return result.status === 0
      },
      update: async (cwd, refName, oid) => {
        await run(cwd, ["update-ref", refName, oid])
      },
      delete: async (cwd, refName) => {
        await run(cwd, ["update-ref", "-d", refName], {
          allowFailure: true,
        })
      },
      getCurrentBranch: async (cwd) => {
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
      getBranchHead: async (cwd, branch) => {
        const result = await run(cwd, ["rev-parse", "--verify", `refs/heads/${branch}`], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
    },
    history: {
      resolveHead: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--verify", "HEAD"], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
      isAncestor: async (cwd, ancestor, descendant) => {
        const result = await run(cwd, ["merge-base", "--is-ancestor", ancestor, descendant], {
          allowFailure: true,
        })
        return result.status === 0
      },
      getMergeBase: async (cwd, left, right) => {
        const result = await run(cwd, ["merge-base", left, right], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
    },
    status: {
      getWorkingTreeStatus: async (cwd) => {
        const result = await run(cwd, ["status", "--porcelain=v1", "--untracked-files=all"])
        const entries = result.stdout
          .split("\n")
          .map((entry) => entry.trimEnd())
          .filter(Boolean)
        return {
          clean: entries.length === 0,
          entries,
        }
      },
      isWorktreeClean: async (cwd) => {
        const status = await createCliGitHost().status.getWorkingTreeStatus(cwd)
        return status.clean
      },
    },
    worktrees: {
      list: async (cwd) => {
        const result = await run(cwd, ["worktree", "list", "--porcelain"])
        return await parseGitWorktrees(result.stdout)
      },
    },
  }
}

export function createLibgit2GitHost(
  fallback: GitHost,
  options: { fallbackOnOperationError?: boolean; libgit2PathCandidates?: string[] } = {},
): GitHost {
  const libgit2 = ensureLibgit2(options.libgit2PathCandidates)

  const runLibgit2 = async <T>(
    operation: () => Promise<T>,
    fallbackOperation: () => Promise<T>,
  ) => {
    if (!options.fallbackOnOperationError) {
      return await operation()
    }

    try {
      return await operation()
    } catch {
      return await fallbackOperation()
    }
  }

  return {
    repository: {
      resolveRoot: (cwd) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, async (repo) => {
              const workdir = libgit2.git_repository_workdir(repo)
              if (!workdir) {
                throw new GitNotRepositoryError(cwd)
              }
              return await normalizePath(String(workdir))
            }),
          () => fallback.repository.resolveRoot(cwd),
        ),
      resolveGitDir: (cwd) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, async (repo) => {
              const gitDir = libgit2.git_repository_path(repo)
              if (!gitDir) {
                throw new GitHostError(`libgit2 could not resolve Git dir for ${cwd}`)
              }
              return await normalizePath(String(gitDir))
            }),
          () => fallback.repository.resolveGitDir(cwd),
        ),
      resolveCommonDir: (cwd) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, async (repo) => {
              const commonDir = libgit2.git_repository_commondir(repo)
              if (!commonDir) {
                throw new GitHostError(`libgit2 could not resolve Git common dir for ${cwd}`)
              }
              return await normalizePath(String(commonDir))
            }),
          () => fallback.repository.resolveCommonDir(cwd),
        ),
      resolveGitPath: (cwd, gitPath) => fallback.repository.resolveGitPath(cwd, gitPath),
      isBareRepository: (cwd) => fallback.repository.isBareRepository(cwd),
    },
    refs: {
      resolve: (cwd, refName) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, (repo) => {
              const object = resolveLibgit2Object(libgit2, repo, refName)
              if (!object) {
                return null
              }

              try {
                const oid = libgit2.git_object_id(object)
                return oid ? String(libgit2.git_oid_tostr_s(oid)) : null
              } finally {
                libgit2.git_object_free(object)
              }
            }),
          () => fallback.refs.resolve(cwd, refName),
        ),
      exists: (cwd, refName) =>
        runLibgit2(
          async () => (await createLibgit2RefResolver(libgit2, cwd, refName)) !== null,
          () => fallback.refs.exists(cwd, refName),
        ),
      update: (cwd, refName, oid) => fallback.refs.update(cwd, refName, oid),
      delete: (cwd, refName) => fallback.refs.delete(cwd, refName),
      getCurrentBranch: (cwd) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, (repo) => {
              const out = pointerStorage()
              const status = libgit2.git_repository_head(ptr(out), repo)
              if (status !== 0) {
                return null
              }

              const head = readPointer(out)
              try {
                const name = libgit2.git_reference_name(head)
                const branchRef = name ? String(name) : ""
                return branchRef.startsWith("refs/heads/")
                  ? branchRef.slice("refs/heads/".length)
                  : null
              } finally {
                libgit2.git_reference_free(head)
              }
            }),
          () => fallback.refs.getCurrentBranch(cwd),
        ),
      branchExists: (cwd, branch) =>
        runLibgit2(
          () =>
            withLibgit2Repository(
              libgit2,
              cwd,
              (repo) =>
                libgit2.git_reference_name_to_id(
                  ptr(new Uint8Array(20)),
                  repo,
                  ptr(cString(`refs/heads/${branch}`)),
                ) === 0,
            ),
          () => fallback.refs.branchExists(cwd, branch),
        ),
      getBranchHead: (cwd, branch) =>
        runLibgit2(
          () => fallback.refs.getBranchHead(cwd, branch),
          () => fallback.refs.getBranchHead(cwd, branch),
        ),
    },
    history: {
      resolveHead: (cwd) =>
        runLibgit2(
          () => fallback.history.resolveHead(cwd),
          () => fallback.history.resolveHead(cwd),
        ),
      isAncestor: (cwd, ancestor, descendant) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, (repo) => {
              const ancestorObject = resolveLibgit2Object(libgit2, repo, ancestor)
              const descendantObject = resolveLibgit2Object(libgit2, repo, descendant)
              if (!ancestorObject || !descendantObject) {
                if (ancestorObject) {
                  libgit2.git_object_free(ancestorObject)
                }
                if (descendantObject) {
                  libgit2.git_object_free(descendantObject)
                }
                return false
              }

              try {
                return (
                  libgit2.git_graph_descendant_of(
                    repo,
                    libgit2.git_object_id(descendantObject),
                    libgit2.git_object_id(ancestorObject),
                  ) === 1
                )
              } finally {
                libgit2.git_object_free(ancestorObject)
                libgit2.git_object_free(descendantObject)
              }
            }),
          () => fallback.history.isAncestor(cwd, ancestor, descendant),
        ),
      getMergeBase: (cwd, left, right) =>
        runLibgit2(
          () =>
            withLibgit2Repository(libgit2, cwd, (repo) => {
              const leftObject = resolveLibgit2Object(libgit2, repo, left)
              const rightObject = resolveLibgit2Object(libgit2, repo, right)
              if (!leftObject || !rightObject) {
                if (leftObject) {
                  libgit2.git_object_free(leftObject)
                }
                if (rightObject) {
                  libgit2.git_object_free(rightObject)
                }
                return null
              }

              try {
                const out = new Uint8Array(20)
                const status = libgit2.git_merge_base(
                  ptr(out),
                  repo,
                  libgit2.git_object_id(leftObject),
                  libgit2.git_object_id(rightObject),
                )
                return status === 0 ? String(libgit2.git_oid_tostr_s(ptr(out))) : null
              } finally {
                libgit2.git_object_free(leftObject)
                libgit2.git_object_free(rightObject)
              }
            }),
          () => fallback.history.getMergeBase(cwd, left, right),
        ),
    },
    status: {
      getWorkingTreeStatus: (cwd) => fallback.status.getWorkingTreeStatus(cwd),
      isWorktreeClean: (cwd) => fallback.status.isWorktreeClean(cwd),
    },
    worktrees: {
      list: (cwd) => fallback.worktrees.list(cwd),
    },
  }
}

function ensureLibgit2(candidates?: string[]) {
  if (!candidates && initializedLibgit2) {
    return initializedLibgit2
  }

  const libgit2 = loadLibgit2(candidates)
  const initStatus = libgit2.symbols.git_libgit2_init()
  if (initStatus < 0) {
    throw new GitHostError(`git_libgit2_init failed with status ${initStatus}`)
  }

  if (candidates) {
    return libgit2.symbols
  }

  initializedLibgit2 = libgit2.symbols
  return initializedLibgit2
}

function loadLibgit2(candidates = libgit2PathCandidates()) {
  const errors: string[] = []
  for (const candidate of candidates) {
    try {
      return dlopen(candidate, {
        git_libgit2_init: {
          args: [],
          returns: FFIType.i32,
        },
        git_repository_open_ext: {
          args: [FFIType.ptr, FFIType.cstring, FFIType.u32, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_repository_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_repository_path: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_workdir: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_commondir: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_head: {
          args: [FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_reference_name: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_reference_name_to_id: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_reference_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_revparse_single: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_object_id: {
          args: [FFIType.ptr],
          returns: FFIType.ptr,
        },
        git_object_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_oid_tostr_s: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_graph_descendant_of: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_merge_base: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_status_list_new: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_status_list_entrycount: {
          args: [FFIType.ptr],
          returns: FFIType.u64,
        },
        git_status_list_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
      })
    } catch (error) {
      errors.push(`${candidate}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  throw new GitHostError(`Unable to load libgit2. Tried: ${errors.join("; ")}`)
}

function libgit2PathCandidates() {
  return [
    process.env.GODDARD_GIT_LIBGIT2_PATH,
    process.env.LIBGIT2_PATH,
    process.env.REVIEW_SYNC_LIBGIT2_PATH,
    `libgit2.${suffix}`,
    `/opt/homebrew/lib/libgit2.${suffix}`,
    `/usr/local/lib/libgit2.${suffix}`,
  ].filter((path) => typeof path === "string")
}

async function createLibgit2RefResolver(libgit2: Libgit2Symbols, cwd: string, refName: string) {
  return await withLibgit2Repository(libgit2, cwd, (repo) => {
    const object = resolveLibgit2Object(libgit2, repo, refName)
    if (!object) {
      return null
    }

    libgit2.git_object_free(object)
    return refName
  })
}

function resolveLibgit2Object(libgit2: Libgit2Symbols, repo: FfiPointer, refName: string) {
  const out = pointerStorage()
  const status = libgit2.git_revparse_single(ptr(out), repo, ptr(cString(refName)))
  return status === 0 ? readPointer(out) : null
}

async function withLibgit2Repository<T>(
  libgit2: Libgit2Symbols,
  cwd: string,
  operation: (repo: FfiPointer) => T | Promise<T>,
) {
  const out = pointerStorage()
  const status = libgit2.git_repository_open_ext(ptr(out), ptr(cString(cwd)), 0, 0)
  if (status !== 0) {
    throw new GitNotRepositoryError(cwd)
  }

  const repo = readPointer(out)
  try {
    return await operation(repo)
  } finally {
    libgit2.git_repository_free(repo)
  }
}

export async function normalizePath(path: string) {
  return await realpath(resolve(path))
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

function resolveGitOutputPath(cwd: string, value: string) {
  return isAbsolute(value) ? value : resolve(cwd, value)
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

export async function runGitCommand(cwd: string, args: string[], options: GitRunOptions = {}) {
  return await new Promise<GitCommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn("git", args, {
      cwd,
      env: {
        ...process.env,
        ...options.env,
      },
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
