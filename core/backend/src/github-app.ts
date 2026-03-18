import { App } from "octokit"
import type { GitHubWebhookInput, RepoEvent } from "@goddard-ai/schema/backend"

type FetchLike = typeof fetch

export type GitHubAppOptions = {
  appId?: string
  privateKey?: string
  webhookSecret?: string
  backendBaseUrl: string
  fetchImpl?: FetchLike
}

export type GitHubWebhookResult = {
  handled: true
  event: RepoEvent
}

export type GoddardGitHubApp = {
  app?: App
  handleWebhook(input: GitHubWebhookInput): Promise<GitHubWebhookResult>
}

export function createGitHubApp(options: GitHubAppOptions): GoddardGitHubApp {
  const baseUrl = new URL(options.backendBaseUrl)
  const fetchImpl = options.fetchImpl ?? fetch
  let app: App | undefined

  if (options.appId && options.privateKey && options.webhookSecret) {
    const githubApp = new App({
      appId: options.appId,
      privateKey: options.privateKey,
      webhooks: {
        secret: options.webhookSecret,
      },
    })
    app = githubApp

    githubApp.webhooks.onAny(async ({ id, name, payload }) => {
      // Prevent infinite loops by ignoring events triggered by bot accounts.
      const sender = (payload as any).sender
      if (sender && sender.type === "Bot") {
        return
      }

      try {
        const body = JSON.stringify(payload)
        await fetchImpl(new URL("/webhooks/github", baseUrl), {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-github-event": name,
            "x-github-delivery": id,
            "x-hub-signature-256": await githubApp.webhooks.sign(body),
          },
          body,
        })
      } catch (error) {
        console.error(`Failed to forward webhook ${name} to backend:`, error)
      }
    })

    githubApp.webhooks.on("issue_comment.created", async ({ octokit, payload }) => {
      if (payload.comment.user?.type === "Bot") {
        return
      }

      try {
        await octokit.rest.reactions.createForIssueComment({
          owner: payload.repository.owner.login,
          repo: payload.repository.name,
          comment_id: payload.comment.id,
          content: "eyes",
        })
      } catch (error) {
        console.error("Failed to add reaction to issue_comment:", error)
      }
    })

    githubApp.webhooks.on("pull_request_review.submitted", async ({ octokit, payload }) => {
      if (payload.review.user?.type === "Bot") {
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
    })

    githubApp.webhooks.on("pull_request", async ({ payload }) => {
      console.log(
        `Received pull_request event: ${payload.action} for PR #${payload.pull_request.number}`,
      )
    })
  }

  const handleWebhook = async (input: GitHubWebhookInput): Promise<GitHubWebhookResult> => {
    const response = await fetchImpl(new URL("/webhooks/github/events", baseUrl), {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify(input),
    })

    if (!response.ok) {
      throw new Error(`Webhook handling failed (${response.status})`)
    }

    const event = (await response.json()) as RepoEvent
    return {
      handled: true,
      event,
    }
  }

  return {
    app,
    handleWebhook,
  }
}
