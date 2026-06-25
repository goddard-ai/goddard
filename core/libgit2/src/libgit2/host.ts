import { GitHostError, GitNotRepositoryError } from "../errors.ts"
import { normalizePath } from "../paths.ts"
import type { GitApi } from "../types.ts"
import {
  createLibgit2RefResolver,
  cString,
  ensureLibgit2,
  pointerStorage,
  readPointer,
  resetLibgit2ForTests,
  resolveLibgit2Object,
  toFfiPointer,
  withLibgit2Repository,
  type Libgit2Symbols,
} from "./ffi.ts"

const namespaceResets: Array<() => void> = []
const unsupportedGit = createUnsupportedGitApi()

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
      unsupportedGit.repository.resolveGitPath(cwd, gitPath),
    isBareRepository: (cwd: string) => unsupportedGit.repository.isBareRepository(cwd),
  }),
  refs: (libgit2) => ({
    resolve: (cwd: string, refName: string) => resolveRefWithLibgit2(libgit2, cwd, refName),
    exists: async (cwd: string, refName: string) =>
      (await createLibgit2RefResolver(libgit2, cwd, refName)) !== null,
    update: (cwd: string, refName: string, oid: string) =>
      unsupportedGit.refs.update(cwd, refName, oid),
    delete: (cwd: string, refName: string) => unsupportedGit.refs.delete(cwd, refName),
    getCurrentBranch: (cwd: string) =>
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
          return branchRef.startsWith("refs/heads/") ? branchRef.slice("refs/heads/".length) : null
        } finally {
          libgit2.git_reference_free(head)
        }
      }),
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
  status: () => unsupportedGit.status,
  worktrees: () => unsupportedGit.worktrees,
  stash: () => unsupportedGit.stash,
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

function createUnsupportedGitApi(): GitApi {
  return {
    repository: {
      resolveRoot: () => unsupported("repository.resolveRoot"),
      resolveGitDir: () => unsupported("repository.resolveGitDir"),
      resolveCommonDir: () => unsupported("repository.resolveCommonDir"),
      resolveGitPath: () => unsupported("repository.resolveGitPath"),
      isBareRepository: () => unsupported("repository.isBareRepository"),
    },
    refs: {
      resolve: () => unsupported("refs.resolve"),
      exists: () => unsupported("refs.exists"),
      update: () => unsupported("refs.update"),
      delete: () => unsupported("refs.delete"),
      getCurrentBranch: () => unsupported("refs.getCurrentBranch"),
      branchExists: () => unsupported("refs.branchExists"),
      getBranchHead: () => unsupported("refs.getBranchHead"),
    },
    history: {
      resolveHead: () => unsupported("history.resolveHead"),
      isAncestor: () => unsupported("history.isAncestor"),
      getMergeBase: () => unsupported("history.getMergeBase"),
    },
    status: {
      getWorkingTreeStatus: () => unsupported("status.getWorkingTreeStatus"),
      isWorktreeClean: () => unsupported("status.isWorktreeClean"),
    },
    worktrees: {
      list: () => unsupported("worktrees.list"),
    },
    stash: {
      list: () => unsupported("stash.list"),
    },
  }
}

async function unsupported<T>(operation: string): Promise<T> {
  throw new GitHostError(`libgit2 host does not support ${operation}`)
}
