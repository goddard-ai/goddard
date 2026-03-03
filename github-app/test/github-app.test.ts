import test from "node:test";
import assert from "node:assert/strict";
import { GoddardGitHubApp } from "../src/index.ts";

test("GoddardGitHubApp initialization", () => {
  const app = new GoddardGitHubApp({
    appId: "123",
    privateKey: "some-key",
    webhookSecret: "secret"
  });
  
  assert.ok(app.app);
  assert.ok(app.app.webhooks);
});
