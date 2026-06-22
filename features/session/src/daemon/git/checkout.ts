import { runGitCommand } from "./command.ts"

export async function checkoutBranch(repoRoot: string, branchName: string) {
  return await runGitCommand(repoRoot, ["checkout", branchName], {
    stdin: "ignore",
  })
}
