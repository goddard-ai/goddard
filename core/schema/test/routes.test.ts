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
