import type { DaemonPullRequest } from "@goddard-ai/pull-request/schema"

/** Returns the canonical browser URL for one daemon-managed GitHub pull request. */
export function getPullRequestUrl(pullRequest: DaemonPullRequest) {
  return `https://github.com/${pullRequest.owner}/${pullRequest.repo}/pull/${pullRequest.prNumber}`
}
