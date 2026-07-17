#!/usr/bin/env bun
import { spawnSync } from "node:child_process"
import { getErrorMessage } from "radashi"

const PASSING_CHECK_RUN_CONCLUSIONS = new Set(["SUCCESS", "SKIPPED", "NEUTRAL"])

type AutoMergeConfig = {
  baseBranch: string
  repo: string
  repoName: string
  repoOwner: string
  userBranch: string
}

type PullRequestSummary = {
  number: number
}

type CheckRun = {
  __typename: "CheckRun"
  conclusion: string | null
  name?: string
  status: string
}

type StatusContext = {
  __typename: "StatusContext"
  context?: string
  state: string
}

type UnknownStatusCheck = {
  __typename: string
}

type StatusCheck = CheckRun | StatusContext | UnknownStatusCheck

type PullRequestView = {
  isDraft: boolean
  mergeStateStatus: string
  statusCheckRollup: StatusCheck[]
}

type ReviewThread = {
  isResolved: boolean
}

type ReviewState = {
  reviewDecision: string | null
  reviewThreads: ReviewThread[]
}

type GraphqlReviewPage = {
  data: {
    repository: {
      pullRequest: {
        reviewDecision: string | null
        reviewThreads: {
          nodes: ReviewThread[]
          pageInfo: {
            endCursor: string | null
            hasNextPage: boolean
          }
        }
      } | null
    }
  }
}

function isCheckRun(check: StatusCheck): check is CheckRun {
  return check.__typename === "CheckRun" && "status" in check && "conclusion" in check
}

function isStatusContext(check: StatusCheck): check is StatusContext {
  return check.__typename === "StatusContext" && "state" in check
}

export type GateSummary = { state: "READY" } | { detail: string; state: "BLOCKED" }

function runGh(args: string[], options?: { stdio?: "pipe" | "inherit" }) {
  const result = spawnSync("gh", args, {
    encoding: "utf8",
    env: {
      ...process.env,
      GH_TOKEN: process.env.GH_TOKEN ?? process.env.GITHUB_TOKEN,
    },
    stdio: options?.stdio ?? "pipe",
  })

  if (result.status !== 0) {
    const detail = result.stderr?.trim() || result.stdout?.trim() || `exit code ${result.status}`
    throw new Error(`gh ${args.join(" ")} failed: ${detail}`)
  }

  return result.stdout
}

function runGhJson<T>(args: string[]) {
  const output = runGh(args).trim()
  return JSON.parse(output || "null") as T
}

function getConfigFromEnv(): AutoMergeConfig {
  const repo = process.env.GITHUB_REPOSITORY
  const repoOwner = process.env.GITHUB_REPOSITORY_OWNER
  const userBranch = process.env.USER_BRANCH
  const baseBranch = process.env.BASE_BRANCH

  if (!repo || !repoOwner || !userBranch || !baseBranch) {
    throw new Error(
      "GITHUB_REPOSITORY, GITHUB_REPOSITORY_OWNER, USER_BRANCH, and BASE_BRANCH are required.",
    )
  }

  const repoName = repo.split("/")[1]

  if (!repoName) {
    throw new Error(`GITHUB_REPOSITORY must be owner/name, got ${repo}.`)
  }

  return {
    baseBranch,
    repo,
    repoName,
    repoOwner,
    userBranch,
  }
}

function findOpenPullRequest(config: AutoMergeConfig) {
  const prs = runGhJson<PullRequestSummary[]>([
    "pr",
    "list",
    "--repo",
    config.repo,
    "--base",
    config.baseBranch,
    "--head",
    config.userBranch,
    "--state",
    "open",
    "--json",
    "number",
  ])

  return prs[0]
}

function getPullRequestView(config: AutoMergeConfig, prNumber: number) {
  return runGhJson<PullRequestView>([
    "pr",
    "view",
    String(prNumber),
    "--repo",
    config.repo,
    "--json",
    "isDraft,mergeStateStatus,statusCheckRollup",
  ])
}

function getReviewState(config: AutoMergeConfig, prNumber: number): ReviewState {
  const reviewThreads: ReviewThread[] = []
  let reviewDecision: string | null = null
  let endCursor: string | null = null

  do {
    const args = [
      "api",
      "graphql",
      "-f",
      `owner=${config.repoOwner}`,
      "-f",
      `name=${config.repoName}`,
      "-F",
      `number=${prNumber}`,
      "-f",
      `query=${reviewThreadsQuery}`,
    ]

    if (endCursor) {
      args.push("-f", `endCursor=${endCursor}`)
    }

    const page = runGhJson<GraphqlReviewPage>(args)
    const pullRequest = page.data.repository.pullRequest

    if (!pullRequest) {
      throw new Error(`PR #${prNumber} was not found.`)
    }

    reviewDecision = pullRequest.reviewDecision
    reviewThreads.push(...pullRequest.reviewThreads.nodes)
    endCursor = pullRequest.reviewThreads.pageInfo.hasNextPage
      ? pullRequest.reviewThreads.pageInfo.endCursor
      : null
  } while (endCursor)

  return { reviewDecision, reviewThreads }
}

export function summarizeStatusChecks(statusCheckRollup: StatusCheck[]) {
  if (statusCheckRollup.length === 0) {
    return "NO_CHECKS"
  }

  const failingCheck = statusCheckRollup.find((check) => {
    if (isCheckRun(check)) {
      return (
        check.status !== "COMPLETED" || !PASSING_CHECK_RUN_CONCLUSIONS.has(check.conclusion ?? "")
      )
    }

    if (isStatusContext(check)) {
      return check.state !== "SUCCESS"
    }

    return true
  })

  return failingCheck ? "NOT_PASSING" : "PASSING"
}

export function summarizeReviewState(reviewState: ReviewState) {
  if (reviewState.reviewDecision === "CHANGES_REQUESTED") {
    return "CHANGES_REQUESTED"
  }

  if (reviewState.reviewThreads.some((thread) => !thread.isResolved)) {
    return "UNRESOLVED_THREADS"
  }

  return "RESOLVED"
}

export function summarizeMergeGate(pr: PullRequestView, reviewState: ReviewState): GateSummary {
  const mergeStateStatus = pr.isDraft ? "DRAFT" : pr.mergeStateStatus

  if (mergeStateStatus !== "CLEAN") {
    return {
      detail: `mergeStateStatus=${mergeStateStatus}`,
      state: "BLOCKED",
    }
  }

  const checkSummary = summarizeStatusChecks(pr.statusCheckRollup)

  if (checkSummary !== "PASSING") {
    return {
      detail: `status checks are ${checkSummary}`,
      state: "BLOCKED",
    }
  }

  const reviewSummary = summarizeReviewState(reviewState)

  if (reviewSummary !== "RESOLVED") {
    return {
      detail: `review feedback is ${reviewSummary}`,
      state: "BLOCKED",
    }
  }

  return { state: "READY" }
}

function mergePullRequest(config: AutoMergeConfig, prNumber: number) {
  runGh(["pr", "merge", String(prNumber), "--repo", config.repo, "--rebase"], { stdio: "inherit" })
}

function main() {
  const config = getConfigFromEnv()
  const pr = findOpenPullRequest(config)

  if (!pr) {
    console.log(`No open PR from ${config.userBranch} to ${config.baseBranch}.`)
    return 0
  }

  const prView = getPullRequestView(config, pr.number)
  const reviewState = getReviewState(config, pr.number)
  const gate = summarizeMergeGate(prView, reviewState)

  if (gate.state !== "READY") {
    console.log(`PR #${pr.number} is not ready to merge; ${gate.detail}.`)
    return 0
  }

  mergePullRequest(config, pr.number)
  return 0
}

const reviewThreadsQuery = `
  query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
    repository(owner: $owner, name: $name) {
      pullRequest(number: $number) {
        reviewDecision
        reviewThreads(first: 100, after: $endCursor) {
          nodes {
            isResolved
          }
          pageInfo {
            hasNextPage
            endCursor
          }
        }
      }
    }
  }
`

if (import.meta.main) {
  try {
    process.exit(main())
  } catch (error) {
    console.error(getErrorMessage(error))
    process.exit(1)
  }
}
