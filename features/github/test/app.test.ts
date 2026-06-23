import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { createGitHubApp } from "../src/backend.ts"

test("GoddardGitHubApp initialization", () => {
  const app = createGitHubApp({
    appId: "123",
    privateKey: "some-key",
    webhookSecret: "secret",
    backendBaseUrl: "http://127.0.0.1:8787",
  })

  expect(app.app).toBeDefined()
  expect(app.app?.webhooks).toBeDefined()
})

test("github-app forwards webhooks to backend and returns handled event", async () => {
  const fetchImpl = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    expect(url.endsWith("/webhooks/github")).toBe(true)

    const body = JSON.parse(String(init?.body))
    expect(init?.headers).toMatchObject({
      "x-github-event": "issue_comment",
      "x-github-delivery": "delivery-1",
    })
    expect(body.action).toBe("created")

    return new Response(
      JSON.stringify({
        name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
        payload: {
          type: "comment",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          author: "teammate",
          body: "nice",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString(),
        },
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    )
  }

  const app = createGitHubApp({
    backendBaseUrl: "http://127.0.0.1:8787",
    fetchImpl: fetchImpl as typeof fetch,
  })
  const result = await app.handleWebhook({
    deliveryId: "delivery-1",
    eventName: "issue_comment",
    payload: {
      action: "created",
      issue: {
        number: 1,
        pull_request: {},
      },
      comment: {
        user: { login: "teammate", type: "User" },
        body: "nice",
      },
      repository: {
        name: "sdk",
        owner: { login: "goddard-ai" },
      },
    },
  })

  expect(result.handled).toBe(true)
  expect(result.event.type).toBe("comment")
})
