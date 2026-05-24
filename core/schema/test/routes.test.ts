import { authBackendRoutes } from "@goddard-ai/auth/backend"
import { pullRequestBackendRoutes } from "@goddard-ai/pull-request/backend"
import { expect, test } from "bun:test"

import { repositories } from "../src/backend/routes.ts"

test("backend routes keep their logical resource grouping", () => {
  const { auth } = authBackendRoutes
  const { pullRequests, webhooks } = pullRequestBackendRoutes

  expect(auth.path.source).toBe("/auth")
  expect(auth.children.device.path.source).toBe("/device")
  expect(auth.children.device.children.start.path?.source).toBe("/start")
  expect(auth.children.device.children.complete.path?.source).toBe("/complete")
  expect(auth.children.session.path.source).toBe("/session")
  expect(auth.children.session.children.current.path?.source).toBe("/current")
  expect(pullRequests.path.source).toBe("/pull-requests")
  expect(pullRequests.children.create.path?.source).toBe("/create")
  expect(pullRequests.children.managed.path?.source).toBe("/managed")
  expect(pullRequests.children.comments.path.source).toBe("/comments")
  expect(pullRequests.children.comments.children.create.path?.source).toBe("/create")
  expect(webhooks.path.source).toBe("/webhooks")
  expect(webhooks.children.github.path?.source).toBe("/github")
  expect(repositories.path.source).toBe("/repositories")
  expect(repositories.children.stream.path?.source).toBe("/stream")
})
