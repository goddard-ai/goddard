import { App } from "octokit";

export type GoddardAppOptions = {
  appId: string;
  privateKey: string;
  webhookSecret: string;
  backendBaseUrl?: string;
};

export class GoddardGitHubApp {
  public readonly app: App;

  constructor(options: GoddardAppOptions) {
    this.app = new App({
      appId: options.appId,
      privateKey: options.privateKey,
      webhooks: {
        secret: options.webhookSecret
      }
    });

    this.app.webhooks.onAny(async ({ id, name, payload }) => {
      if (options.backendBaseUrl) {
        try {
          await fetch(`${options.backendBaseUrl}/webhooks/github`, {
            method: "POST",
            headers: {
              "content-type": "application/json",
              "x-github-event": name,
              "x-github-delivery": id
            },
            body: JSON.stringify(payload)
          });
        } catch (error) {
          console.error(`Failed to forward webhook ${name} to backend:`, error);
        }
      }
    });

    this.app.webhooks.on("issue_comment.created", async ({ octokit, payload }) => {
      if (payload.comment.user?.type === "Bot") {
        return;
      }

      try {
        await octokit.rest.reactions.createForIssueComment({
          owner: payload.repository.owner.login,
          repo: payload.repository.name,
          comment_id: payload.comment.id,
          content: "eyes"
        });
      } catch (error) {
        console.error("Failed to add reaction to issue_comment:", error);
      }
    });

    this.app.webhooks.on("pull_request_review.submitted", async ({ octokit, payload }) => {
      if (payload.review.user?.type === "Bot") {
        return;
      }

      try {
        await octokit.request(
          "POST /repos/{owner}/{repo}/pulls/{pull_number}/reviews/{review_id}/reactions",
          {
            owner: payload.repository.owner.login,
            repo: payload.repository.name,
            pull_number: payload.pull_request.number,
            review_id: payload.review.id,
            content: "eyes"
          }
        );
      } catch (error) {
        console.error("Failed to add reaction to pull_request_review:", error);
      }
    });

    this.app.webhooks.on("pull_request", async ({ octokit, payload }) => {
      // Just receiving the event as specified in build.md.
      // Automated responses or logging can be added here.
      console.log(`Received pull_request event: ${payload.action} for PR #${payload.pull_request.number}`);
    });
  }
}
