import { authDeviceComplete, authDeviceStart, authSession } from "@goddard-ai/auth/backend"
import { githubWebhook, prCreate, prManaged } from "@goddard-ai/pull-request/backend"
import { expect, test } from "bun:test"

import { repoStream } from "../src/backend/routes.ts"

test("backend routes keep their stable public paths", () => {
  expect(authDeviceStart.path?.source).toBe("/auth/device/start")
  expect(authDeviceComplete.path?.source).toBe("/auth/device/complete")
  expect(authSession.path?.source).toBe("/auth/session")
  expect(prCreate.path?.source).toBe("/pr/create")
  expect(prManaged.path?.source).toBe("/pr/managed")
  expect(githubWebhook.path?.source).toBe("/webhooks/github")
  expect(repoStream.path?.source).toBe("/stream")
})
