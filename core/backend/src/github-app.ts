import type { GitHubWebhookDeliveryInput } from "@goddard-ai/github/schema"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { App } from "octokit"

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
  handleWebhook(input: GitHubWebhookDeliveryInput): Promise<GitHubWebhookResult>
}

export function createGitHubApp(options: GitHubAppOptions): GoddardGitHubApp {
  const baseUrl = new URL(options.backendBaseUrl)
  const fetchImpl = options.fetchImpl ?? fetch
  let app: App | undefined

  if (options.appId && options.privateKey && options.webhookSecret) {
    app = new App({
      appId: options.appId,
      privateKey: options.privateKey,
      webhooks: {
        secret: options.webhookSecret,
      },
    })

    app.webhooks.onAny(async ({ id, name, payload }) => {
      // Prevent infinite loops by ignoring events triggered by bot accounts.
      const sender = (payload as any).sender
      if (sender && sender.type === "Bot") {
        return
      }

      const delivery = normalizeGitHubAppWebhook(id, name, payload)
      if (!delivery) {
        return
      }

      try {
        const body = JSON.stringify(delivery)
        await fetchImpl(new URL("/webhooks/github", baseUrl), {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-github-event": name,
            "x-github-delivery": id,
            ...(options.webhookSecret
              ? { "x-hub-signature-256": await signGitHubWebhookBody(options.webhookSecret, body) }
              : {}),
          },
          body,
        })
      } catch (error) {
        console.error(`Failed to forward webhook ${name} to backend:`, error)
      }
    })

    app.webhooks.on("issue_comment.created", async ({ octokit, payload }) => {
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

    app.webhooks.on("pull_request_review.submitted", async ({ octokit, payload }) => {
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

    app.webhooks.on("pull_request", async ({ payload }) => {
      console.log(
        `Received pull_request event: ${payload.action} for PR #${payload.pull_request.number}`,
      )
    })
  }

  const handleWebhook = async (input: GitHubWebhookDeliveryInput): Promise<GitHubWebhookResult> => {
    const body = JSON.stringify(input)
    const response = await fetchImpl(new URL("/webhooks/github", baseUrl), {
      method: "POST",
      headers: {
        "content-type": "application/json",
        ...(options.webhookSecret
          ? { "x-hub-signature-256": await signGitHubWebhookBody(options.webhookSecret, body) }
          : {}),
      },
      body,
    })

    if (!response.ok) {
      throw new Error(`Webhook handling failed (${response.status})`)
    }

    const envelope = (await response.json()) as { payload: RepoEvent }
    return {
      handled: true,
      event: envelope.payload,
    }
  }

  return {
    app,
    handleWebhook,
  }
}

function normalizeGitHubAppWebhook(
  deliveryId: string,
  eventName: string,
  payload: any,
): GitHubWebhookDeliveryInput | undefined {
  if (eventName === "issue_comment" && payload.action === "created") {
    if (!payload.issue?.pull_request) {
      return undefined
    }

    return {
      deliveryId,
      event: {
        type: "issue_comment",
        owner: payload.repository.owner.login,
        repo: payload.repository.name,
        prNumber: payload.issue.number,
        author: payload.comment.user.login,
        body: payload.comment.body ?? "",
      },
    }
  }

  if (eventName === "pull_request_review" && payload.action === "submitted") {
    const state = String(payload.review.state).toLowerCase()
    if (!["approved", "changes_requested", "commented"].includes(state)) {
      return undefined
    }

    return {
      deliveryId,
      event: {
        type: "pull_request_review",
        owner: payload.repository.owner.login,
        repo: payload.repository.name,
        prNumber: payload.pull_request.number,
        author: payload.review.user.login,
        state: state as "approved" | "changes_requested" | "commented",
        body: payload.review.body ?? "",
      },
    }
  }

  return undefined
}

async function signGitHubWebhookBody(secret: string, body: string): Promise<string> {
  const encoder = new TextEncoder()
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  )
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(body))
  const hex = [...new Uint8Array(signature)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("")
  return `sha256=${hex}`
}
