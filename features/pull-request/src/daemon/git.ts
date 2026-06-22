import { parseGitHubRepositoryUrl } from "@goddard-ai/github/daemon"
import { createGitHost } from "@goddard-ai/libgit2"

import type { ReplyPrRequest, SubmitPrRequest } from "../schema.ts"
import { readOriginHeadRef, readOriginRemoteUrl } from "./git/config.ts"

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

type RepositoryUrlParser = (remote: string) =>
  | {
      provider: string
      owner: string
      repo: string
    }
  | undefined

/** Resolves PR creation defaults from the local Git checkout. */
export async function resolveSubmitRequestFromGit(
  input: SubmitPrRequest,
  parseRepositoryUrl: RepositoryUrlParser = parseGitHubRepositoryUrl,
): Promise<PrCreateInput> {
  const { provider, owner, repo } = await inferRepoFromGit(input.cwd, parseRepositoryUrl)

  return {
    provider,
    owner,
    repo,
    title: input.title,
    body: input.body,
    head: input.head || (await inferCurrentBranch(input.cwd)),
    base: input.base || (await inferBaseBranch(input.cwd)),
  }
}

/** Resolves PR reply defaults from the local Git checkout. */
export async function resolveReplyRequestFromGit(
  input: ReplyPrRequest,
  parseRepositoryUrl: RepositoryUrlParser = parseGitHubRepositoryUrl,
): Promise<PrReplyInput> {
  const { provider, owner, repo } = await inferRepoFromGit(input.cwd, parseRepositoryUrl)

  return {
    provider,
    owner,
    repo,
    prNumber: input.prNumber ?? (await inferPrNumberFromGit(input.cwd)),
    body: input.message,
  }
}

async function inferRepoFromGit(cwd: string, parseRepositoryUrl: RepositoryUrlParser) {
  const remote = await readOriginRemoteUrl(cwd)
  const repository = parseRepositoryUrl(remote)
  if (repository) {
    return repository
  }

  throw new Error(`Unsupported origin remote URL: ${remote}`)
}

async function inferCurrentBranch(cwd: string): Promise<string> {
  return (await createGitHost().refs.getCurrentBranch(cwd)) ?? "HEAD"
}

async function inferBaseBranch(cwd: string): Promise<string> {
  try {
    const headRef = await readOriginHeadRef(cwd)
    return headRef.replace(/^refs\/remotes\/origin\//, "") || "main"
  } catch {
    return "main"
  }
}

async function inferPrNumberFromGit(cwd: string): Promise<number> {
  const branch = await inferCurrentBranch(cwd)
  const match = branch.match(/^pr-(\d+)$/)
  if (!match) {
    throw new Error("Unable to infer PR number from current branch. Expected pr-<number>.")
  }

  return Number.parseInt(match[1], 10)
}
