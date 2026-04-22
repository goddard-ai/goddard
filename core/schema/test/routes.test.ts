import { expect, test } from "bun:test"

import {
  authDeviceCompleteRoute,
  authDeviceStartRoute,
  authSessionRoute,
  githubWebhookRoute,
  prCreateRoute,
  prManagedRoute,
  repoStreamRoute,
} from "../src/backend/routes.ts"

test("backend routes keep their stable public paths", () => {
  expect(authDeviceStartRoute.path.source).toBe("auth/device/start")
  expect(authDeviceCompleteRoute.path.source).toBe("auth/device/complete")
  expect(authSessionRoute.path.source).toBe("auth/session")
  expect(prCreateRoute.path.source).toBe("pr/create")
  expect(prManagedRoute.path.source).toBe("pr/managed")
  expect(githubWebhookRoute.path.source).toBe("webhooks/github")
  expect(repoStreamRoute.path.source).toBe("stream")
})
