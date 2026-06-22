import { resetLibgit2ForTests } from "./libgit2/ffi.ts"
import { createLibgit2GitHost } from "./libgit2/host.ts"
import type { GitHostOptions } from "./types.ts"

export function createGitHost(options: GitHostOptions = {}) {
  return createLibgit2GitHost({
    libgit2PathCandidates: options.libgit2PathCandidates,
  })
}

export function resetGitHostForTests() {
  resetLibgit2ForTests()
}
