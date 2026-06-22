import { runGitCommand } from "./command.ts"

export async function readOriginRef(cwd: string, refName: string) {
  const result = await runGitCommand(cwd, ["symbolic-ref", `refs/remotes/origin/${refName}`])
  if (result.status !== 0) {
    const stderr = result.stderr.trim()
    throw new Error(stderr || `git symbolic-ref refs/remotes/origin/${refName} failed in ${cwd}`)
  }

  return result.stdout.trim()
}
