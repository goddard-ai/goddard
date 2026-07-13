import { join, resolve } from "node:path"

import { GitHostError, GitNotRepositoryError } from "../errors.ts"
import { normalizePath } from "../paths.ts"
import {
  createLibgit2RefResolver,
  cString,
  ensureLibgit2,
  pointerStorage,
  readCString,
  readNestedPointer,
  readPointer,
  readUint32,
  readUint64,
  resetLibgit2ForTests,
  resolveLibgit2Object,
  toFfiPointer,
  withLibgit2Repository,
  type FfiPointer,
  type Libgit2Symbols,
} from "./ffi.ts"

const namespaceResets: Array<() => void> = []

export const git = defineGitNamespaces({
  repository: (libgit2) => ({
    resolveRoot: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, async (repo) => {
        const workdir = libgit2.git_repository_workdir(repo)
        if (!workdir) {
          throw new GitNotRepositoryError(cwd)
        }
        return await normalizePath(String(workdir))
      }),
    resolveGitDir: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, async (repo) => {
        const gitDir = libgit2.git_repository_path(repo)
        if (!gitDir) {
          throw new GitHostError(`libgit2 could not resolve Git dir for ${cwd}`)
        }
        return await normalizePath(String(gitDir))
      }),
    resolveCommonDir: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, async (repo) => {
        const commonDir = libgit2.git_repository_commondir(repo)
        if (!commonDir) {
          throw new GitHostError(`libgit2 could not resolve Git common dir for ${cwd}`)
        }
        return await normalizePath(String(commonDir))
      }),
    resolveGitPath: (cwd: string, gitPath: string) =>
      withLibgit2Repository(libgit2, cwd, async (repo) => {
        const gitDir = libgit2.git_repository_path(repo)
        const commonDir = libgit2.git_repository_commondir(repo)
        if (!gitDir || !commonDir) {
          throw new GitHostError(`libgit2 could not resolve Git path for ${cwd}`)
        }

        const root = commonGitPathRoots.has(gitPath.split("/")[0] ?? "") ? commonDir : gitDir
        return join(await normalizePath(String(root)), gitPath)
      }),
    isBareRepository: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => libgit2.git_repository_is_bare(repo) === 1),
  }),
  refs: (libgit2) => ({
    resolve: (cwd: string, refName: string) => resolveRefWithLibgit2(libgit2, cwd, refName),
    exists: async (cwd: string, refName: string) =>
      (await createLibgit2RefResolver(libgit2, cwd, refName)) !== null,
    update: (cwd: string, refName: string, oid: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const parsedOid = new Uint8Array(20)
        if (libgit2.git_oid_fromstr(toFfiPointer(parsedOid), toFfiPointer(cString(oid))) !== 0) {
          throw new GitHostError(`Invalid Git object ID: ${oid}`)
        }

        const out = pointerStorage()
        const status = libgit2.git_reference_create(
          toFfiPointer(out),
          repo,
          toFfiPointer(cString(refName)),
          toFfiPointer(parsedOid),
          1,
          0,
        )
        if (status !== 0) {
          throw new GitHostError(`libgit2 could not update ref ${refName} (status ${status})`)
        }

        libgit2.git_reference_free(readPointer(out))
      }),
    delete: (cwd: string, refName: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const out = pointerStorage()
        const lookupStatus = libgit2.git_reference_lookup(
          toFfiPointer(out),
          repo,
          toFfiPointer(cString(refName)),
        )
        if (lookupStatus !== 0) {
          return
        }

        const reference = readPointer(out)
        try {
          const deleteStatus = libgit2.git_reference_delete(reference)
          if (deleteStatus !== 0) {
            throw new GitHostError(
              `libgit2 could not delete ref ${refName} (status ${deleteStatus})`,
            )
          }
        } finally {
          libgit2.git_reference_free(reference)
        }
      }),
    getCurrentBranch: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => getCurrentBranch(libgit2, repo)),
    branchExists: (cwd: string, branch: string) =>
      withLibgit2Repository(
        libgit2,
        cwd,
        (repo) =>
          libgit2.git_reference_name_to_id(
            toFfiPointer(new Uint8Array(20)),
            repo,
            toFfiPointer(cString(`refs/heads/${branch}`)),
          ) === 0,
      ),
    getBranchHead: (cwd: string, branch: string) =>
      resolveRefWithLibgit2(libgit2, cwd, `refs/heads/${branch}`),
  }),
  history: (libgit2) => ({
    resolveHead: (cwd: string) => resolveRefWithLibgit2(libgit2, cwd, "HEAD"),
    isAncestor: (cwd: string, ancestor: string, descendant: string) =>
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
          const ancestorOid = libgit2.git_object_id(ancestorObject)
          const descendantOid = libgit2.git_object_id(descendantObject)
          if (
            String(libgit2.git_oid_tostr_s(ancestorOid)) ===
            String(libgit2.git_oid_tostr_s(descendantOid))
          ) {
            return true
          }

          return libgit2.git_graph_descendant_of(repo, descendantOid, ancestorOid) === 1
        } finally {
          libgit2.git_object_free(ancestorObject)
          libgit2.git_object_free(descendantObject)
        }
      }),
    getMergeBase: (cwd: string, left: string, right: string) =>
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
            toFfiPointer(out),
            repo,
            libgit2.git_object_id(leftObject),
            libgit2.git_object_id(rightObject),
          )
          return status === 0 ? String(libgit2.git_oid_tostr_s(toFfiPointer(out))) : null
        } finally {
          libgit2.git_object_free(leftObject)
          libgit2.git_object_free(rightObject)
        }
      }),
  }),
  status: (libgit2) => ({
    getWorkingTreeStatus: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const statusList = createStatusList(libgit2, repo)
        try {
          const count = Number(libgit2.git_status_list_entrycount(statusList))
          const entries = Array.from({ length: count }, (_, index) => {
            const entry = libgit2.git_status_byindex(statusList, index)
            if (!entry) {
              throw new GitHostError(`libgit2 returned an invalid status entry at index ${index}`)
            }
            return formatStatusEntry(entry)
          })
          return { clean: entries.length === 0, entries }
        } finally {
          libgit2.git_status_list_free(statusList)
        }
      }),
    isWorktreeClean: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const statusList = createStatusList(libgit2, repo)
        try {
          return Number(libgit2.git_status_list_entrycount(statusList)) === 0
        } finally {
          libgit2.git_status_list_free(statusList)
        }
      }),
  }),
  worktrees: (libgit2) => ({
    list: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const commonDir = libgit2.git_repository_commondir(repo)
        if (!commonDir) {
          throw new GitHostError(`libgit2 could not resolve Git common dir for ${cwd}`)
        }
        return withLibgit2Repository(libgit2, String(commonDir), (mainRepo) =>
          listWorktrees(libgit2, mainRepo),
        )
      }),
  }),
  stash: (libgit2) => ({
    list: (cwd: string) =>
      withLibgit2Repository(libgit2, cwd, (repo) => {
        const out = pointerStorage()
        const status = libgit2.git_reflog_read(
          toFfiPointer(out),
          repo,
          toFfiPointer(cString("refs/stash")),
        )
        if (status !== 0) {
          if (status === -3) {
            return new Map<string, string>()
          }
          throw new GitHostError(`git_reflog_read failed with status ${status}`)
        }

        const reflog = readPointer(out)
        try {
          const count = Number(libgit2.git_reflog_entrycount(reflog))
          return new Map(
            Array.from({ length: count }, (_, index) => {
              const entry = libgit2.git_reflog_entry_byindex(reflog, index)
              const message = entry ? libgit2.git_reflog_entry_message(entry) : null
              return [`stash@{${index}}`, message ? String(message) : ""]
            }),
          )
        } finally {
          libgit2.git_reflog_free(reflog)
        }
      }),
  }),
})

export function validateLibgit2Runtime(options: { libgit2PathCandidates?: string[] } = {}) {
  ensureLibgit2(options.libgit2PathCandidates)
}

export function resetGitForTests() {
  resetLibgit2ForTests()
  for (const reset of namespaceResets) {
    reset()
  }
}

type GitNamespaceFactories = Record<string, (libgit2: Libgit2Symbols) => object>

function defineGitNamespaces<T extends GitNamespaceFactories>(
  factories: T,
): { readonly [K in keyof T]: ReturnType<T[K]> } {
  type GitNamespaces = { readonly [K in keyof T]: ReturnType<T[K]> }
  let namespaces: GitNamespaces | undefined

  const load = () => {
    if (!namespaces) {
      const libgit2 = ensureLibgit2()
      const loaded = {} as { -readonly [K in keyof T]: ReturnType<T[K]> }
      for (const key of Object.keys(factories) as Array<keyof T>) {
        loaded[key] = factories[key](libgit2) as ReturnType<T[typeof key]>
      }
      namespaces = loaded
    }

    return namespaces
  }

  namespaceResets.push(() => {
    namespaces = undefined
  })

  const api = {} as GitNamespaces
  const descriptors: PropertyDescriptorMap = {}
  for (const key of Object.keys(factories) as Array<keyof T>) {
    descriptors[String(key)] = {
      enumerable: true,
      configurable: false,
      get() {
        return load()[key]
      },
    }
  }
  Object.defineProperties(api, descriptors)
  return api
}

function resolveRefWithLibgit2(libgit2: Libgit2Symbols, cwd: string, refName: string) {
  return withLibgit2Repository(libgit2, cwd, (repo) => {
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
  })
}

function getCurrentBranch(libgit2: Libgit2Symbols, repo: FfiPointer) {
  const out = pointerStorage()
  const status = libgit2.git_repository_head(toFfiPointer(out), repo)
  if (status !== 0) {
    return null
  }

  const head = readPointer(out)
  try {
    const name = libgit2.git_reference_name(head)
    const branchRef = name ? String(name) : ""
    return branchRef.startsWith("refs/heads/") ? branchRef.slice("refs/heads/".length) : null
  } finally {
    libgit2.git_reference_free(head)
  }
}

async function listWorktrees(libgit2: Libgit2Symbols, repo: FfiPointer) {
  const entries = []
  const mainPath = libgit2.git_repository_workdir(repo)
  if (mainPath) {
    entries.push({
      path: resolve(String(mainPath)),
      branch: getCurrentBranch(libgit2, repo),
    })
  }

  const names = new Uint8Array(16)
  const status = libgit2.git_worktree_list(toFfiPointer(names), repo)
  if (status !== 0) {
    throw new GitHostError(`git_worktree_list failed with status ${status}`)
  }

  try {
    const namesPointer = readNestedPointer(toFfiPointer(names))
    const count = Number(readUint64(toFfiPointer(names), 8))
    for (let index = 0; index < count; index += 1) {
      const namePointer = readNestedPointer(namesPointer, index * 8)
      const out = pointerStorage()
      const lookupStatus = libgit2.git_worktree_lookup(toFfiPointer(out), repo, namePointer)
      if (lookupStatus !== 0) {
        throw new GitHostError(`git_worktree_lookup failed with status ${lookupStatus}`)
      }

      const worktree = readPointer(out)
      try {
        const worktreePath = libgit2.git_worktree_path(worktree)
        if (!worktreePath) {
          throw new GitHostError(
            `libgit2 returned no path for worktree ${readCString(namePointer)}`,
          )
        }
        const path = resolve(String(worktreePath))
        entries.push({
          path,
          branch: await withLibgit2Repository(libgit2, path, (worktreeRepo) =>
            getCurrentBranch(libgit2, worktreeRepo),
          ),
        })
      } finally {
        libgit2.git_worktree_free(worktree)
      }
    }
  } finally {
    libgit2.git_strarray_dispose(toFfiPointer(names))
  }

  return entries
}

const commonGitPathRoots = new Set([
  "config",
  "hooks",
  "info",
  "logs",
  "modules",
  "objects",
  "packed-refs",
  "refs",
  "remotes",
  "worktrees",
])

// libgit2 v1's git_status_options is 48 bytes on the supported 64-bit targets.
const statusOptionBytes = 48
const statusIncludeUntracked = 1 << 0
const statusRecurseUntrackedDirectories = 1 << 4

function createStatusList(libgit2: Libgit2Symbols, repo: FfiPointer) {
  const options = new Uint8Array(statusOptionBytes)
  const optionsStatus = libgit2.git_status_options_init(toFfiPointer(options), 1)
  if (optionsStatus !== 0) {
    throw new GitHostError(`git_status_options_init failed with status ${optionsStatus}`)
  }
  new DataView(options.buffer).setUint32(
    8,
    statusIncludeUntracked | statusRecurseUntrackedDirectories,
    true,
  )

  const out = pointerStorage()
  const status = libgit2.git_status_list_new(toFfiPointer(out), repo, toFfiPointer(options))
  if (status !== 0) {
    throw new GitHostError(`git_status_list_new failed with status ${status}`)
  }
  return readPointer(out)
}

function formatStatusEntry(entry: FfiPointer) {
  const status = readUint32(entry)
  const headToIndex = readNestedPointer(entry, 8)
  const indexToWorkdir = readNestedPointer(entry, 16)
  const path = readStatusPath(indexToWorkdir || headToIndex)
  if ((status & (1 << 15)) !== 0) {
    return `UU ${path}`
  }
  if ((status & (1 << 7)) !== 0 && (status & 0x1f) === 0) {
    return `?? ${path}`
  }

  return `${formatIndexStatus(status)}${formatWorkdirStatus(status)} ${path}`
}

function readStatusPath(delta: FfiPointer) {
  if (!delta) {
    return ""
  }
  // git_diff_delta embeds old_file and new_file; their path pointers are at these ABI offsets.
  const newPath = readNestedPointer(delta, 88)
  const oldPath = readNestedPointer(delta, 40)
  const path = newPath || oldPath
  return path ? readCString(path) : ""
}

function formatIndexStatus(status: number) {
  if ((status & (1 << 3)) !== 0) return "R"
  if ((status & (1 << 4)) !== 0) return "T"
  if ((status & (1 << 0)) !== 0) return "A"
  if ((status & (1 << 2)) !== 0) return "D"
  if ((status & (1 << 1)) !== 0) return "M"
  return " "
}

function formatWorkdirStatus(status: number) {
  if ((status & (1 << 11)) !== 0) return "R"
  if ((status & (1 << 10)) !== 0) return "T"
  if ((status & (1 << 9)) !== 0) return "D"
  if ((status & (1 << 8)) !== 0) return "M"
  if ((status & (1 << 12)) !== 0) return "?"
  return " "
}
