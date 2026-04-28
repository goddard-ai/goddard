import { runGit } from "./command"

/** Reads porcelain working tree status without mutating the repository. */
export async function getWorkingTreeStatus(rootDir: string) {
  const entries = (await runGit(rootDir, ["status", "--porcelain=v1"]))
    .split("\n")
    .map((entry) => entry.trimEnd())
    .filter(Boolean)

  return {
    clean: entries.length === 0,
    entries,
  }
}
