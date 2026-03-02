import test from "node:test";
import assert from "node:assert/strict";
import { createGitHubApp } from "../src/index.ts";

test("github-app forwards webhooks to backend and returns handled event", async () => {
  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input);
    assert.ok(url.endsWith("/webhooks/github"));

    const body = JSON.parse(String(init?.body));
    assert.equal(body.type, "issue_comment");

    return new Response(
      JSON.stringify({
        type: "comment",
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: 1,
        author: "teammate",
        body: "nice",
        reactionAdded: "eyes",
        createdAt: new Date().toISOString()
      }),
      { status: 200, headers: { "content-type": "application/json" } }
    );
  };

  const app = createGitHubApp({ backendBaseUrl: "http://127.0.0.1:8787", fetchImpl });
  const result = await app.handleWebhook({
    type: "issue_comment",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 1,
    author: "teammate",
    body: "nice"
  });

  assert.equal(result.handled, true);
  assert.equal(result.event.type, "comment");
});
