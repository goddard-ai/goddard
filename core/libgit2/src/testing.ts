import type { GitHost } from "./index.ts"

type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends (...args: never[]) => unknown ? T[K] : DeepPartial<T[K]>
}

export function createFakeGitHost(overrides: DeepPartial<GitHost> = {}): GitHost {
  const fail = async () => {
    throw new Error("Fake Git host method was not implemented for this test.")
  }

  return {
    repository: {
      resolveRoot: fail,
      resolveGitDir: fail,
      resolveCommonDir: fail,
      resolveGitPath: fail,
      isBareRepository: fail,
      ...overrides.repository,
    },
    refs: {
      resolve: fail,
      exists: fail,
      update: fail,
      delete: fail,
      getCurrentBranch: fail,
      branchExists: fail,
      getBranchHead: fail,
      ...overrides.refs,
    },
    history: {
      resolveHead: fail,
      isAncestor: fail,
      getMergeBase: fail,
      ...overrides.history,
    },
    status: {
      getWorkingTreeStatus: fail,
      isWorktreeClean: fail,
      ...overrides.status,
    },
    worktrees: {
      list: fail,
      ...overrides.worktrees,
    },
    stash: {
      list: fail,
      ...overrides.stash,
    },
  }
}
