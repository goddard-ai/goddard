import type { DaemonPullRequest } from "@goddard-ai/schema/daemon"

/** Returns the compact title used for daemon-managed pull request tabs and rows. */
export function getPullRequestDisplayTitle(pullRequest: DaemonPullRequest) {
  return `${pullRequest.owner}/${pullRequest.repo} #${pullRequest.prNumber}`
}
