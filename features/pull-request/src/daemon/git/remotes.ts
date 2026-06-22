import { runGitCommand } from "./command.ts"

export async function readOriginRemoteUrl(cwd: string) {
  return await runGit(cwd, ["config", "--get", "remote.origin.url"])
}

async function runGit(cwd: string, args: string[]): Promise<string> {
  const result = await runGitCommand(cwd, args)
  if (result.status !== 0) {
    const stderr = result.stderr.trim()
    throw new Error(stderr || `git ${args.join(" ")} failed in ${cwd}`)
  }

  return result.stdout.trim()
}
