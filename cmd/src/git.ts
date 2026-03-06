import { readFile } from "node:fs/promises"

export async function inferRepoFromGitConfig(path = ".git/config"): Promise<string | null> {
  let gitConfig = ""
  try {
    gitConfig = await readFile(path, "utf-8")
  } catch {
    return null
  }

  const match = gitConfig.match(/\[remote "origin"\][\s\S]*?url\s*=\s*(.+)/)
  if (!match) {
    return null
  }

  const remote = match[1].trim()
  const httpsMatch = remote.match(/^https:\/\/github\.com\/(.+?)\/(.+?)(\.git)?$/)
  if (httpsMatch) {
    return `${httpsMatch[1]}/${httpsMatch[2]}`
  }

  const sshMatch = remote.match(/^git@github\.com:(.+?)\/(.+?)(\.git)?$/)
  if (sshMatch) {
    return `${sshMatch[1]}/${sshMatch[2]}`
  }

  return null
}

import { spawnSync } from "node:child_process"

export function inferPrNumberFromGit(dir = process.cwd()): number | null {
  const result = spawnSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], {
    cwd: dir,
    encoding: "utf-8",
  })

  if (result.status !== 0) {
    return null
  }

  const branch = result.stdout.trim()
  const match = branch.match(/^pr-(\d+)$/)
  if (match) {
    return parseInt(match[1], 10)
  }

  return null
}

export function splitRepo(repoRef: string): { owner: string; repo: string } {
  const [owner, repo] = repoRef.split("/")
  if (!owner || !repo) {
    throw new Error("repo must look like owner/repo")
  }
  return { owner, repo }
}
