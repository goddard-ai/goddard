import { relative } from "node:path"
import { git } from "@goddard-ai/libgit2"

export async function isGitIgnoredDirectory(params: { gitRoot: string; path: string }) {
  const relativePath = relative(params.gitRoot, params.path)

  if (relativePath.length === 0 || relativePath === ".." || relativePath.startsWith("../")) {
    return false
  }

  return await git.ignore.isIgnored(params.gitRoot, `${relativePath}/`)
}

export async function filterGitignoredPaths(repoRoot: string, relativePaths: string[]) {
  if (relativePaths.length === 0) {
    return new Set<string>()
  }

  const normalizedPaths = relativePaths.map((relativePath) =>
    trimTrailingPathSeparator(relativePath),
  )
  return await git.ignore.filterIgnored(repoRoot, normalizedPaths)
}

function trimTrailingPathSeparator(relativePath: string) {
  return relativePath.endsWith("/") || relativePath.endsWith("\\")
    ? relativePath.slice(0, -1)
    : relativePath
}
