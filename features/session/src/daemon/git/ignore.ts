import { relative } from "node:path"

import { runGitCommand } from "./command.ts"

export async function isGitIgnoredDirectory(params: { gitRoot: string; path: string }) {
  const relativePath = relative(params.gitRoot, params.path)

  if (relativePath.length === 0 || relativePath === ".." || relativePath.startsWith("../")) {
    return false
  }

  const result = await runGitCommand(
    params.gitRoot,
    ["check-ignore", "-q", "--", `${relativePath}/`],
    {
      stdin: "ignore",
    },
  )
  return result.status === 0
}

export async function filterGitignoredPaths(repoRoot: string, relativePaths: string[]) {
  if (relativePaths.length === 0) {
    return new Set<string>()
  }

  const normalizedPaths = relativePaths.map((relativePath) =>
    trimTrailingPathSeparator(relativePath),
  )
  const result = await runGitCommand(repoRoot, ["check-ignore", "--stdin", "-z"], {
    stdin: `${normalizedPaths.join("\0")}\0`,
  })

  if (result.status !== 0 && result.status !== 1) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git check-ignore failed")
  }

  return new Set(parseGitPathOutput(result.stdout).map(trimTrailingPathSeparator))
}

function parseGitPathOutput(stdout: string) {
  return stdout
    .split("\0")
    .filter((line) => line.length > 0)
    .map(trimTrailingPathSeparator)
}

function trimTrailingPathSeparator(relativePath: string) {
  return relativePath.endsWith("/") || relativePath.endsWith("\\")
    ? relativePath.slice(0, -1)
    : relativePath
}
