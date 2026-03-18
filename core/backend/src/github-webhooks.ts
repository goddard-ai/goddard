import type {
  GitHubWebhookInput,
  GitHubWebhookReceipt,
  RepoEvent,
} from "@goddard-ai/schema/backend"
import { Webhooks } from "@octokit/webhooks"
import { App } from "octokit"
import { HttpError } from "./api/control-plane.ts"
import type { Env } from "./env.ts"

// Dependencies required to verify a GitHub webhook delivery and fan it out internally.
type GitHubWebhookHandlerOptions = {
  env: Env
  request: Request
  handleWebhookEvent(input: GitHubWebhookInput): Promise<RepoEvent> | RepoEvent
  broadcastEvent(event: RepoEvent): Promise<void>
}

// Review states that map directly into the repo-event contract.
type SupportedReviewState = "approved" | "changes_requested" | "commented"

export async function handleGitHubWebhookRequest(
  options: GitHubWebhookHandlerOptions,
): Promise<GitHubWebhookReceipt> {
  if (!options.env.GITHUB_WEBHOOK_SECRET) {
    throw new HttpError(500, "GitHub webhook secret is not configured on the backend")
  }

  const signature = options.request.headers.get("x-hub-signature-256")
  const name = options.request.headers.get("x-github-event")
  const id = options.request.headers.get("x-github-delivery")

  if (!signature || !name || !id) {
    throw new HttpError(400, "Missing required GitHub webhook headers")
  }

  let receipt: GitHubWebhookReceipt = { handled: false }
  const webhooks = new Webhooks({ secret: options.env.GITHUB_WEBHOOK_SECRET })

  webhooks.on("issue_comment.created", async ({ payload }) => {
    if (payload.comment.user?.type === "Bot" || payload.sender?.type === "Bot") {
      return
    }

    if (!payload.issue.pull_request) {
      return
    }

    await addIssueCommentReaction(options.env, payload)

    const event = await options.handleWebhookEvent({
      type: "issue_comment",
      owner: payload.repository.owner.login,
      repo: payload.repository.name,
      prNumber: payload.issue.number,
      author: payload.comment.user?.login ?? payload.sender.login,
      body: payload.comment.body ?? "",
    })

    await options.broadcastEvent(event)
    receipt = { handled: true, event }
  })

  webhooks.on("pull_request_review.submitted", async ({ payload }) => {
    if (payload.review.user?.type === "Bot" || payload.sender?.type === "Bot") {
      return
    }

    const state = toReviewState(payload.review.state)
    if (!state) {
      return
    }

    await addPullRequestReviewReaction(options.env, payload)

    const event = await options.handleWebhookEvent({
      type: "pull_request_review",
      owner: payload.repository.owner.login,
      repo: payload.repository.name,
      prNumber: payload.pull_request.number,
      author: payload.review.user?.login ?? payload.sender.login,
      state,
      body: payload.review.body ?? "",
    })

    await options.broadcastEvent(event)
    receipt = { handled: true, event }
  })

  try {
    await webhooks.verifyAndReceive({
      id,
      name,
      payload: await options.request.text(),
      signature,
    })
  } catch (error) {
    throw toWebhookHttpError(error)
  }

  return receipt
}

async function addIssueCommentReaction(
  env: Env,
  payload: {
    installation?: { id: number } | null
    repository: { owner: { login: string }; name: string }
    comment: { id: number }
  },
): Promise<void> {
  const octokit = await getInstallationOctokit(env, payload.installation?.id)
  if (!octokit) {
    return
  }

  try {
    await octokit.request("POST /repos/{owner}/{repo}/issues/comments/{comment_id}/reactions", {
      owner: payload.repository.owner.login,
      repo: payload.repository.name,
      comment_id: payload.comment.id,
      content: "eyes",
    })
  } catch (error) {
    console.error("Failed to add reaction to issue_comment:", error)
  }
}

async function addPullRequestReviewReaction(
  env: Env,
  payload: {
    installation?: { id: number } | null
    repository: { owner: { login: string }; name: string }
    pull_request: { number: number }
    review: { id: number }
  },
): Promise<void> {
  const octokit = await getInstallationOctokit(env, payload.installation?.id)
  if (!octokit) {
    return
  }

  try {
    await octokit.request(
      "POST /repos/{owner}/{repo}/pulls/{pull_number}/reviews/{review_id}/reactions",
      {
        owner: payload.repository.owner.login,
        repo: payload.repository.name,
        pull_number: payload.pull_request.number,
        review_id: payload.review.id,
        content: "eyes",
      },
    )
  } catch (error) {
    console.error("Failed to add reaction to pull_request_review:", error)
  }
}

async function getInstallationOctokit(
  env: Env,
  installationId: number | undefined,
): Promise<Awaited<ReturnType<App["getInstallationOctokit"]>> | undefined> {
  if (!env.GITHUB_APP_ID || !env.GITHUB_APP_PRIVATE_KEY || !installationId) {
    return undefined
  }

  return new App({
    appId: env.GITHUB_APP_ID,
    privateKey: env.GITHUB_APP_PRIVATE_KEY,
  }).getInstallationOctokit(installationId)
}

function toReviewState(state: string): SupportedReviewState | null {
  if (state === "approved" || state === "changes_requested" || state === "commented") {
    return state
  }

  return null
}

function toWebhookHttpError(error: unknown): HttpError {
  if (!(error instanceof AggregateError) || error.errors.length === 0) {
    return new HttpError(
      500,
      error instanceof Error ? error.message : "GitHub webhook handling failed",
    )
  }

  const [firstError] = error.errors
  return new HttpError(
    typeof firstError?.status === "number" ? firstError.status : 500,
    firstError instanceof Error ? firstError.message : "GitHub webhook handling failed",
  )
}
