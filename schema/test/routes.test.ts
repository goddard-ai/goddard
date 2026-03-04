import test from "node:test";
import assert from "node:assert/strict";
import {
  authDeviceStartRoute,
  authDeviceCompleteRoute,
  authSessionRoute,
  prCreateRoute,
  githubWebhookRoute,
  repoStreamRoute
} from "../src/index.ts";

test("schema exports rouzer route declarations with stable paths", () => {
  assert.equal(authDeviceStartRoute.path.source, "auth/device/start");
  assert.equal(authDeviceCompleteRoute.path.source, "auth/device/complete");
  assert.equal(authSessionRoute.path.source, "auth/session");
  assert.equal(prCreateRoute.path.source, "pr/create");
  assert.equal(githubWebhookRoute.path.source, "webhooks/github");
  assert.equal(repoStreamRoute.path.source, "stream");
});
