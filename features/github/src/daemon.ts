export type GitHubRepositoryUrl = {
  readonly provider: "github"
  readonly owner: string
  readonly repo: string
}

export function parseGitHubRepositoryUrl(remote: string): GitHubRepositoryUrl | undefined {
  const httpsMatch = remote.match(/^https:\/\/github\.com\/(.+?)\/(.+?)(\.git)?$/)
  if (httpsMatch) {
    return {
      provider: "github",
      owner: httpsMatch[1],
      repo: httpsMatch[2],
    }
  }

  const sshMatch = remote.match(/^git@github\.com:(.+?)\/(.+?)(\.git)?$/)
  if (sshMatch) {
    return {
      provider: "github",
      owner: sshMatch[1],
      repo: sshMatch[2],
    }
  }

  return undefined
}
