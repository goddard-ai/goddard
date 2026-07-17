import type { GitApi } from "./index.ts"

type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends (...args: never[]) => unknown ? T[K] : DeepPartial<T[K]>
}

/** Creates an in-memory Git API whose non-overridden operations fail when called. */
export function createFakeGitApi(overrides: DeepPartial<GitApi> = {}): GitApi {
  const fail = async () => {
    throw new Error("Fake Git API method was not implemented for this test.")
  }

  return {
    repository: {
      resolveRoot: fail,
      resolveGitDir: fail,
      resolveCommonDir: fail,
      isBareRepository: fail,
      ...overrides.repository,
    },
    refs: {
      resolve: fail,
      update: fail,
      delete: fail,
      getCurrentBranch: fail,
      branchExists: fail,
      readSymbolic: fail,
      listLocalBranches: fail,
      ...overrides.refs,
    },
    history: {
      resolveHead: fail,
      isAncestor: fail,
      getMergeBase: fail,
      countCommits: fail,
      ...overrides.history,
    },
    status: {
      getWorkingTreeStatus: fail,
      isWorktreeClean: fail,
      listUntracked: fail,
      ...overrides.status,
    },
    config: {
      get: fail,
      ...overrides.config,
    },
    ignore: {
      isIgnored: fail,
      filterIgnored: fail,
      ...overrides.ignore,
    },
    index: {
      listPaths: fail,
      ...overrides.index,
    },
    worktrees: {
      list: fail,
      ...overrides.worktrees,
    },
  }
}
