import {
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
} from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { getDefaultBackendPluginComposition } from "../src/backend.ts"
import { daemonIpcRoutes } from "../src/daemon-ipc.ts"
import { getDefaultDaemonPluginComposition } from "../src/daemon.ts"

test("default daemon composition includes file search", () => {
  expect(Object.hasOwn(daemonIpcRoutes, "fileSearch")).toBe(true)
  expect(
    getDefaultDaemonPluginComposition().plugins.some((plugin) => plugin.name === "file-search"),
  ).toBe(true)
})

test("default backend composition includes backend feature routes and remote-repo events", () => {
  const composition = getDefaultBackendPluginComposition()

  expect(composition.routes.auth.path.source).toBe("/auth")
  expect(composition.routes.webhooks.children.github.path?.source).toBe("/github")
  expect(composition.routes.pullRequests.path.source).toBe("/pull-requests")
  expect(composition.routes.events.children.stream.path?.source).toBe("/stream")
  expect(Object.keys(composition.events)).toEqual([
    REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
    REMOTE_REPO_PULL_REQUEST_CREATED,
  ])
  expect(Object.keys(composition.eventSources)).toEqual(["remote-repo"])
  expect(Object.keys(composition.providers)).toEqual(["github"])
  expect(composition.providers.github.authorizeRemoteRepositoryAccess).toBeFunction()
  expect(composition.providers.github.createPullRequest).toBeFunction()
  expect(composition.providers.github.createPullRequestComment).toBeFunction()
  expect(composition.eventSources["remote-repo"].produces).toEqual([
    REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
    REMOTE_REPO_PULL_REQUEST_CREATED,
  ])
})
