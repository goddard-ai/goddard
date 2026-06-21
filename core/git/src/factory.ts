import { createCliGitHost } from "./cli/host.ts"
import { resetLibgit2ForTests } from "./libgit2/ffi.ts"
import { createLibgit2GitHost } from "./libgit2/host.ts"
import type { GitHostOptions } from "./types.ts"

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
  if (env.GODDARD_GIT_LIBGIT2_PATH) {
    return "libgit2"
  }
  return "auto"
}

export function resetGitHostForTests() {
  resetLibgit2ForTests()
}
