import {
  defineBackendProviders,
  type BackendProviderCapabilityDefinition,
  type BackendProviderPrincipal,
  type BackendProviderPullRequestCommentInput,
  type BackendProviderPullRequestCreateInput,
  type BackendProviderRepositoryRef,
} from "@goddard-ai/backend-plugin"
import { App } from "octokit"

export type GitHubProviderEnv = {
  readonly GITHUB_APP_ID?: string
  readonly GITHUB_APP_PRIVATE_KEY?: string
}

export class GitHubProviderError extends Error {
  constructor(
    readonly statusCode: number,
    message: string,
  ) {
    super(message)
  }
}

export const githubBackendProviders = defineBackendProviders({
  github: {
    authorizeRemoteRepositoryAccess: ({ principal, repository }) =>
      canGitHubPrincipalAccessRepository(principal, repository),
    createPullRequest: createPullRequestWithGitHubApp,
    createPullRequestComment: createPullRequestCommentWithGitHubApp,
  } satisfies BackendProviderCapabilityDefinition<"github">,
})

export async function createPullRequestWithGitHubApp(
  input: BackendProviderPullRequestCreateInput<"github">,
) {
  const octokit = await createInstallationOctokit(input.env, input.owner, input.repo)

  try {
    const { data } = await octokit.rest.pulls.create({
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body: input.body,
      head: input.head,
      base: input.base,
    })

    return {
      number: data.number,
      url: data.html_url,
      createdAt: data.created_at,
    }
  } catch (error) {
    throw new GitHubProviderError(
      500,
      `Failed to create pull request on GitHub: ${getErrorMessage(error)}`,
    )
  }
}

export async function createPullRequestCommentWithGitHubApp(
  input: BackendProviderPullRequestCommentInput<"github">,
): Promise<void> {
  const octokit = await createInstallationOctokit(input.env, input.owner, input.repo)

  try {
    await octokit.rest.issues.createComment({
      owner: input.owner,
      repo: input.repo,
      issue_number: input.prNumber,
      body: input.body,
    })
  } catch (error) {
    throw new GitHubProviderError(
      500,
      `Failed to post comment to GitHub: ${getErrorMessage(error)}`,
    )
  }
}

export function canGitHubPrincipalAccessRepository(
  principal: BackendProviderPrincipal,
  repository: BackendProviderRepositoryRef<"github">,
) {
  if (repository.provider !== "github") {
    return false
  }

  const hasGitHubIdentity = principal.providerIdentities.some(
    (identity) => identity.provider === "github",
  )
  if (!hasGitHubIdentity) {
    return false
  }

  return (
    principal.repositories?.some(
      (allowed) =>
        allowed.provider === "github" &&
        allowed.owner === repository.owner &&
        allowed.repo === repository.repo,
    ) ?? false
  )
}

/** Resolves the GitHub App installation that grants backend authority for one repository. */
async function createInstallationOctokit(env: unknown, owner: string, repo: string) {
  const githubEnv = readGitHubProviderEnv(env)
  if (!githubEnv.GITHUB_APP_ID || !githubEnv.GITHUB_APP_PRIVATE_KEY) {
    throw new GitHubProviderError(500, "GitHub App credentials are not configured on the backend")
  }

  const app = new App({
    appId: githubEnv.GITHUB_APP_ID,
    privateKey: githubEnv.GITHUB_APP_PRIVATE_KEY,
  })

  try {
    const { data } = await app.octokit.request("GET /repos/{owner}/{repo}/installation", {
      owner,
      repo,
    })
    return app.getInstallationOctokit(data.id)
  } catch {
    throw new GitHubProviderError(500, `Failed to get GitHub App installation for ${owner}/${repo}`)
  }
}

function readGitHubProviderEnv(env: unknown): GitHubProviderEnv {
  if (!env || typeof env !== "object") {
    return {}
  }

  const record = env as Record<string, unknown>
  return {
    GITHUB_APP_ID: typeof record.GITHUB_APP_ID === "string" ? record.GITHUB_APP_ID : undefined,
    GITHUB_APP_PRIVATE_KEY:
      typeof record.GITHUB_APP_PRIVATE_KEY === "string" ? record.GITHUB_APP_PRIVATE_KEY : undefined,
  }
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
