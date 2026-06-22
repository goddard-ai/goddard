import { runGitCommand } from "./command.ts"

export async function listLocalBranches(sourcePath: string) {
  const result = await runGitCommand(sourcePath, [
    "for-each-ref",
    "--format=%(if)%(HEAD)%(then)*%(end)%(refname:short)",
    "refs/heads",
  ])
  if (result.status !== 0) {
    return { branches: [], currentBranch: null }
  }

  const branches: string[] = []
  let currentBranch: string | null = null

  for (const rawLine of result.stdout.split("\n")) {
    const line = rawLine.trim()
    if (!line) {
      continue
    }

    const current = line.startsWith("*")
    const name = current ? line.slice(1) : line
    if (current) {
      currentBranch = name
    }

    branches.push(name)
  }

  return {
    branches,
    currentBranch,
  }
}
