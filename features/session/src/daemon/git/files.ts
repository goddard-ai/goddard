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
