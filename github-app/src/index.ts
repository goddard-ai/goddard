import { App } from "octokit"
import type { GitHubWebhookInput, RepoEvent } from "@goddard-ai/sdk"

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

export class GoddardGitHubApp {
  public readonly app?: App
  readonly #baseUrl: URL
  readonly #fetchImpl: FetchLike

  constructor(options: GitHubAppOptions) {
    this.#baseUrl = new URL(options.backendBaseUrl)
    this.#fetchImpl = options.fetchImpl ?? fetch

    if (options.appId && options.privateKey && options.webhookSecret) {
      this.app = new App({
        appId: options.appId,
        privateKey: options.privateKey,
        webhooks: {
          secret: options.webhookSecret,
        },
      })

      this.app.webhooks.onAny(async ({ id, name, payload }) => {
        // Prevent infinite loops by ignoring events triggered by bot accounts.
        const sender = (payload as any).sender
        if (sender && sender.type === "Bot") {
          return
        }

        try {
          await this.#fetchImpl(new URL("/webhooks/github", this.#baseUrl), {
            method: "POST",
            headers: {
              "content-type": "application/json",
              "x-github-event": name,
              "x-github-delivery": id,
            },
            body: JSON.stringify(payload),
          })
        } catch (error) {
          console.error(`Failed to forward webhook ${name} to backend:`, error)
        }
      })

      this.app.webhooks.on("issue_comment.created", async ({ octokit, payload }) => {
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

      this.app.webhooks.on("pull_request_review.submitted", async ({ octokit, payload }) => {
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

      this.app.webhooks.on("pull_request", async ({ payload }) => {
        console.log(
          `Received pull_request event: ${payload.action} for PR #${payload.pull_request.number}`,
        )
      })
    }
  }

  async handleWebhook(input: GitHubWebhookInput): Promise<GitHubWebhookResult> {
    const response = await this.#fetchImpl(new URL("/webhooks/github", this.#baseUrl), {
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
}

export function createGitHubApp(options: GitHubAppOptions): GoddardGitHubApp {
  return new GoddardGitHubApp(options)
}
