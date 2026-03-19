export function splitRepo(repoRef: string): { owner: string; repo: string } {
  const [owner, repo] = repoRef.split("/")
  if (!owner || !repo) {
    throw new Error("repo must be in owner/repo format")
  }
  return { owner, repo }
}
