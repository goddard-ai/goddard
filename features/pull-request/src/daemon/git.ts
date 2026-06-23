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
  const remote = await runGit(cwd, ["config", "--get", "remote.origin.url"])
  const repository = parseRepositoryUrl(remote)
  if (repository) {
    return repository
  }

  throw new Error(`Unsupported origin remote URL: ${remote}`)
}

async function inferCurrentBranch(cwd: string): Promise<string> {
  return runGit(cwd, ["rev-parse", "--abbrev-ref", "HEAD"])
}

async function inferBaseBranch(cwd: string): Promise<string> {
  try {
    const headRef = await runGit(cwd, ["symbolic-ref", "refs/remotes/origin/HEAD"])
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

async function runGit(cwd: string, args: string[]): Promise<string> {
  const subprocess = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })
  const [exitCode, stdout, stderr] = await Promise.all([
    subprocess.exited,
    new Response(subprocess.stdout).text(),
    new Response(subprocess.stderr).text(),
  ])

  if (exitCode !== 0) {
    const message = stderr.trim()
    throw new Error(message || `git ${args.join(" ")} failed in ${cwd}`)
  }

  return stdout.trim()
}
