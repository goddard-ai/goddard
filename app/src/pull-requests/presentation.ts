import type { DaemonPullRequest } from "@goddard-ai/schema/daemon"

/** Returns the compact title used for daemon-managed pull request tabs and rows. */
export function getPullRequestDisplayTitle(pullRequest: DaemonPullRequest) {
  return `${pullRequest.owner}/${pullRequest.repo} #${pullRequest.prNumber}`
}

/** Returns the canonical browser URL for one daemon-managed GitHub pull request. */
export function getPullRequestUrl(pullRequest: DaemonPullRequest) {
  return `https://github.com/${pullRequest.owner}/${pullRequest.repo}/pull/${pullRequest.prNumber}`
}
