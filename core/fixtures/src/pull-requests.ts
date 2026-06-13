import type { DaemonPullRequest, GetPullRequestResponse } from "@goddard-ai/pull-request/schema"

import { fixturePullRequestId } from "./ids.ts"
import { fixtureNow, fixtureProjectPath } from "./time.ts"

export function createFixturePullRequest(
  overrides: Partial<DaemonPullRequest> = {},
): DaemonPullRequest {
  const prNumber = overrides.prNumber ?? 128

  return {
    id: overrides.id ?? fixturePullRequestId(prNumber),
    cwd: fixtureProjectPath,
    host: "github",
    owner: "goddard-ai",
    prNumber,
    repo: "goddard-ai",
    updatedAt: fixtureNow,
    ...overrides,
  }
}

export function createGetPullRequestResponse(
  pullRequest: DaemonPullRequest = createFixturePullRequest(),
): GetPullRequestResponse {
  return { pullRequest }
}
