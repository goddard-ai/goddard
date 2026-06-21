import { createCliGitHost } from "../cli/host.ts"
import { GitHostError, GitNotRepositoryError } from "../errors.ts"
import { normalizePath } from "../paths.ts"
import type { GitHost } from "../types.ts"
import {
  createLibgit2RefResolver,
  cString,
  ensureLibgit2,
  pointerStorage,
  readPointer,
  resolveLibgit2Object,
  toFfiPointer,
  withLibgit2Repository,
} from "./ffi.ts"

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
              const status = libgit2.git_repository_head(toFfiPointer(out), repo)
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
                  toFfiPointer(new Uint8Array(20)),
                  repo,
                  toFfiPointer(cString(`refs/heads/${branch}`)),
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
    stash: {
      list: (cwd) => fallback.stash.list(cwd),
    },
  }
}

export function validateLibgit2Runtime(options: { libgit2PathCandidates?: string[] } = {}) {
  createLibgit2GitHost(createCliGitHost(), {
    libgit2PathCandidates: options.libgit2PathCandidates,
  })
}
