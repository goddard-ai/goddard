import type { DaemonPullRequest } from "@goddard-ai/schema/daemon"

/** Returns the canonical browser URL for one daemon-managed GitHub pull request. */
export function getPullRequestUrl(pullRequest: DaemonPullRequest) {
  return `https://github.com/${pullRequest.owner}/${pullRequest.repo}/pull/${pullRequest.prNumber}`
}
