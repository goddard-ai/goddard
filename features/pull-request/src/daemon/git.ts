import { parseGitHubRepositoryUrl } from "@goddard-ai/github/daemon"

import type { ReplyPrRequest, SubmitPrRequest } from "../schema.ts"

type PrCreateInput = {
  provider: string
  owner: string
  repo: string
  title: string
  body?: string
  head: string
  base: string
}

type PrReplyInput = {
  provider: string
  owner: string
  repo: string
  prNumber: number
  body: string
}

/** Resolves PR creation defaults from the local Git checkout. */
export async function resolveSubmitRequestFromGit(input: SubmitPrRequest): Promise<PrCreateInput> {
  const { provider, owner, repo } = inferRepoFromGit(input.cwd)

  return {
    provider,
    owner,
    repo,
    title: input.title,
    body: input.body,
    head: input.head || inferCurrentBranch(input.cwd),
    base: input.base || inferBaseBranch(input.cwd),
  }
}

/** Resolves PR reply defaults from the local Git checkout. */
export async function resolveReplyRequestFromGit(input: ReplyPrRequest): Promise<PrReplyInput> {
  const { provider, owner, repo } = inferRepoFromGit(input.cwd)

  return {
    provider,
    owner,
    repo,
    prNumber: input.prNumber ?? inferPrNumberFromGit(input.cwd),
    body: input.message,
  }
}

function inferRepoFromGit(cwd: string) {
  const remote = runGit(cwd, ["config", "--get", "remote.origin.url"])
  const repository = parseGitHubRepositoryUrl(remote)
  if (repository) {
    return repository
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
