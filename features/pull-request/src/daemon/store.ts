import { kind } from "kindstore"

import { DaemonPullRequest } from "../schema.ts"

/** Daemon persistence owned by the pull request feature. */
export const pullRequestDbSchema = {
  pullRequests: kind("pr", DaemonPullRequest).updatedAt().multi(
    "host_owner_repo_prNumber",
    {
      host: "asc",
      owner: "asc",
      repo: "asc",
      prNumber: "asc",
    },
    { unique: true },
  ),
}
