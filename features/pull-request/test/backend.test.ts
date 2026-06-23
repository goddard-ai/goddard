import { expect, test } from "bun:test"

import { pullRequestBackendPlugin } from "../src/backend.ts"

test("pull-request backend plugin exposes pull-request routes only", () => {
  expect(pullRequestBackendPlugin.name).toBe("pull-request")
  expect(pullRequestBackendPlugin.routes?.pullRequests.path.source).toBe("/pull-requests")
  expect("events" in pullRequestBackendPlugin).toBe(false)
  expect("eventSources" in pullRequestBackendPlugin).toBe(false)
})
