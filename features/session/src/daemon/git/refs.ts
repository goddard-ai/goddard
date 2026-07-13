import { git, GitNotRepositoryError } from "@goddard-ai/libgit2"

export async function listLocalBranches(sourcePath: string) {
  try {
    return {
      branches: await git.refs.listLocalBranches(sourcePath),
      currentBranch: await git.refs.getCurrentBranch(sourcePath),
    }
  } catch (error) {
    if (error instanceof GitNotRepositoryError) {
      return { branches: [], currentBranch: null }
    }
    throw error
  }
}
