import { test, assert } from "vitest"
import {
  authDeviceStartRoute,
  authDeviceCompleteRoute,
  authSessionRoute,
  prCreateRoute,
  prManagedRoute,
  githubWebhookRoute,
  repoStreamRoute,
} from "../src/backend/routes.ts"

test("backend routes keep their stable public paths", () => {
  assert.equal(authDeviceStartRoute.path.source, "auth/device/start")
  assert.equal(authDeviceCompleteRoute.path.source, "auth/device/complete")
  assert.equal(authSessionRoute.path.source, "auth/session")
  assert.equal(prCreateRoute.path.source, "pr/create")
  assert.equal(prManagedRoute.path.source, "pr/managed")
  assert.equal(githubWebhookRoute.path.source, "webhooks/github")
  assert.equal(repoStreamRoute.path.source, "stream")
})

test("backend auth and PR routes parse representative request contracts", () => {
  assert.deepEqual(authDeviceStartRoute.methods.POST?.body?.parse({ githubUsername: "alec" }), {
    githubUsername: "alec",
  })
  assert.deepEqual(
    authDeviceCompleteRoute.methods.POST?.body?.parse({
      deviceCode: "dev_1",
      githubUsername: "alec",
    }),
    {
      deviceCode: "dev_1",
      githubUsername: "alec",
    },
  )
  assert.deepEqual(
    authSessionRoute.methods.GET?.headers?.parse({ authorization: "Bearer tok_1" }),
    {
      authorization: "Bearer tok_1",
    },
  )
  assert.deepEqual(
    prCreateRoute.methods.POST?.body?.parse({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add CLI",
      head: "feat/cli",
      base: "main",
    }),
    {
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add CLI",
      head: "feat/cli",
      base: "main",
    },
  )
  assert.deepEqual(
    prManagedRoute.methods.GET?.query?.parse({
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: "12",
    }),
    {
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 12,
    },
  )
})

test("backend stream and webhook routes parse representative contracts", () => {
  assert.deepEqual(
    repoStreamRoute.methods.GET?.query?.parse({ owner: "goddard-ai", repo: "sdk" }),
    {
      owner: "goddard-ai",
      repo: "sdk",
    },
  )
  assert.deepEqual(
    githubWebhookRoute.methods.POST?.body?.parse({
      type: "pull_request_review",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 42,
      author: "teammate",
      state: "changes_requested",
      body: "Please tighten the parser.",
    }),
    {
      type: "pull_request_review",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 42,
      author: "teammate",
      state: "changes_requested",
      body: "Please tighten the parser.",
    },
  )
})
