import { expect, test } from "bun:test"
import { createGitHubApp } from "../src/github-app.ts"

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
    expect(body.type).toBe("issue_comment")

    return new Response(
      JSON.stringify({
        type: "comment",
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: 1,
        author: "teammate",
        body: "nice",
        reactionAdded: "eyes",
        createdAt: new Date().toISOString(),
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    )
  }

  const app = createGitHubApp({
    backendBaseUrl: "http://127.0.0.1:8787",
    fetchImpl: fetchImpl as typeof fetch,
  })
  const result = await app.handleWebhook({
    type: "issue_comment",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 1,
    author: "teammate",
    body: "nice",
  })

  expect(result.handled).toBe(true)
  expect(result.event.type).toBe("comment")
})
