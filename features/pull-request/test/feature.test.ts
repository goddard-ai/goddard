import { describe, expect, test } from "bun:test"

import { pullRequestIpcSchema } from "../src/daemon-ipc.ts"
import { pullRequestPlugin } from "../src/daemon.ts"
import { GetPullRequestRequest } from "../src/schema.ts"
import { pullRequestSdkPlugin } from "../src/sdk.ts"

describe("pull-request feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(pullRequestPlugin.name).toBe("pull-request")
    expect(Object.keys(pullRequestIpcSchema.requests)).toEqual(["pr.submit", "pr.get", "pr.reply"])
    expect(pullRequestSdkPlugin.namespace).toBe("pullRequest")
    expect(GetPullRequestRequest.parse({ id: "pr_1" })).toEqual({ id: "pr_1" })
  })
})
