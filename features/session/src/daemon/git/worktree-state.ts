import { runGitCommand } from "./command.ts"

export async function countCommitsAhead(cwd: string, baseOid: string) {
  const result = await runGitCommand(cwd, ["rev-list", "--count", `${baseOid}..HEAD`])
  if (result.status !== 0) {
    throw new Error("Unable to inspect worktree commits")
  }

  return Number(result.stdout.trim())
}
