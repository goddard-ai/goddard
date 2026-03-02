import { readFile } from "node:fs/promises";

export async function inferRepoFromGitConfig(path = ".git/config"): Promise<string | null> {
  let gitConfig = "";
  try {
    gitConfig = await readFile(path, "utf-8");
  } catch {
    return null;
  }

  const match = gitConfig.match(/\[remote "origin"\][\s\S]*?url\s*=\s*(.+)/);
  if (!match) {
    return null;
  }

  const remote = match[1].trim();
  const httpsMatch = remote.match(/^https:\/\/github\.com\/(.+?)\/(.+?)(\.git)?$/);
  if (httpsMatch) {
    return `${httpsMatch[1]}/${httpsMatch[2]}`;
  }

  const sshMatch = remote.match(/^git@github\.com:(.+?)\/(.+?)(\.git)?$/);
  if (sshMatch) {
    return `${sshMatch[1]}/${sshMatch[2]}`;
  }

  return null;
}

export function splitRepo(repoRef: string): { owner: string; repo: string } {
  const [owner, repo] = repoRef.split("/");
  if (!owner || !repo) {
    throw new Error("repo must look like owner/repo");
  }
  return { owner, repo };
}
