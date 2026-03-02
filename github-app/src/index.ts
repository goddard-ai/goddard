import type { GitHubWebhookInput, RepoEvent } from "@goddard-ai/sdk";

type FetchLike = typeof fetch;

export type GitHubAppOptions = {
  backendBaseUrl: string;
  fetchImpl?: FetchLike;
};

export type GitHubWebhookResult = {
  handled: true;
  event: RepoEvent;
};

export class GoddardGitHubApp {
  readonly #baseUrl: URL;
  readonly #fetchImpl: FetchLike;

  constructor(options: GitHubAppOptions) {
    this.#baseUrl = new URL(options.backendBaseUrl);
    this.#fetchImpl = options.fetchImpl ?? fetch;
  }

  async handleWebhook(input: GitHubWebhookInput): Promise<GitHubWebhookResult> {
    const response = await this.#fetchImpl(new URL("/webhooks/github", this.#baseUrl), {
      method: "POST",
      headers: {
        "content-type": "application/json"
      },
      body: JSON.stringify(input)
    });

    if (!response.ok) {
      throw new Error(`Webhook handling failed (${response.status})`);
    }

    const event = (await response.json()) as RepoEvent;
    return {
      handled: true,
      event
    };
  }
}

export function createGitHubApp(options: GitHubAppOptions): GoddardGitHubApp {
  return new GoddardGitHubApp(options);
}
