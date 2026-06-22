import { runGitCommand } from "./command.ts"

export type UntrackedEntry = {
  relativePath: string
  isDir: boolean
}

export async function listUntrackedEntries(repoRoot: string): Promise<UntrackedEntry[]> {
  const result = await runGitCommand(
    repoRoot,
    ["ls-files", "--others", "--exclude-standard", "--directory"],
    { stdin: "ignore" },
  )

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git ls-files failed")
  }

  return result.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const isDir = line.endsWith("/")
      return {
        relativePath: isDir ? line.slice(0, -1) : line,
        isDir,
      }
    })
}

export async function listUntrackedEntriesMatchedByExcludeFile(
  repoRoot: string,
  excludeFile: string,
) {
  const result = await runGitCommand(
    repoRoot,
    ["ls-files", "--others", "--ignored", "--directory", "-z", `--exclude-from=${excludeFile}`],
    { stdin: "ignore" },
  )

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git ls-files failed")
  }

  return parseGitPathOutput(result.stdout).map((relativePath) => ({ relativePath }))
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
