import { splitRepo } from "../utils.ts"
import type {
  PrCreateInput,
  PrReplyInput,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "./types.ts"

export async function resolveSubmitRequestFromGit(
  input: SubmitPrDaemonRequest,
): Promise<PrCreateInput> {
  const repoRef = inferRepoFromGit(input.cwd)
  const { owner, repo } = splitRepo(repoRef)

  return {
    owner,
    repo,
    title: input.title,
    body: input.body,
    head: input.head || inferCurrentBranch(input.cwd),
    base: input.base || inferBaseBranch(input.cwd),
  }
}

export async function resolveReplyRequestFromGit(
  input: ReplyPrDaemonRequest,
): Promise<PrReplyInput> {
  const repoRef = inferRepoFromGit(input.cwd)
  const { owner, repo } = splitRepo(repoRef)

  return {
    owner,
    repo,
    prNumber: input.prNumber ?? inferPrNumberFromGit(input.cwd),
    body: input.message,
  }
}

function inferRepoFromGit(cwd: string): string {
  const remote = runGit(cwd, ["config", "--get", "remote.origin.url"])
  const httpsMatch = remote.match(/^https:\/\/github\.com\/(.+?)\/(.+?)(\.git)?$/)
  if (httpsMatch) {
    return `${httpsMatch[1]}/${httpsMatch[2]}`
  }

  const sshMatch = remote.match(/^git@github\.com:(.+?)\/(.+?)(\.git)?$/)
  if (sshMatch) {
    return `${sshMatch[1]}/${sshMatch[2]}`
  }

  throw new Error(`Unsupported origin remote URL: ${remote}`)
}

function inferCurrentBranch(cwd: string): string {
  return runGit(cwd, ["rev-parse", "--abbrev-ref", "HEAD"])
}

function inferBaseBranch(cwd: string): string {
  try {
    const headRef = runGit(cwd, ["symbolic-ref", "refs/remotes/origin/HEAD"])
    return headRef.replace(/^refs\/remotes\/origin\//, "") || "main"
  } catch {
    return "main"
  }
}

function inferPrNumberFromGit(cwd: string): number {
  const branch = inferCurrentBranch(cwd)
  const match = branch.match(/^pr-(\d+)$/)
  if (!match) {
    throw new Error("Unable to infer PR number from current branch. Expected pr-<number>.")
  }

  return Number.parseInt(match[1], 10)
}

function runGit(cwd: string, args: string[]): string {
  const result = Bun.spawnSync(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })

  if (!result.success) {
    const stderr = Buffer.from(result.stderr).toString("utf8").trim()
    throw new Error(stderr || `git ${args.join(" ")} failed in ${cwd}`)
  }

  return Buffer.from(result.stdout).toString("utf8").trim()
}
