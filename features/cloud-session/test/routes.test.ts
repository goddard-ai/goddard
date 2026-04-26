import { expect, test } from "bun:test"

import { cloudSessionBackendRoutes } from "../src/backend.ts"

test("backend routes keep their stable public paths", () => {
  expect(cloudSessionBackendRoutes.cloudSessionCreateRoute.path.source).toBe("cloud/sessions")
  expect(cloudSessionBackendRoutes.cloudSessionCreateByIdRoute.path.source).toBe(
    "cloud/sessions/:sessionId",
  )
  expect(cloudSessionBackendRoutes.cloudSessionSyncRoute.path.source).toBe(
    "cloud/sessions/:sessionId/sync",
  )
  expect(cloudSessionBackendRoutes.cloudSessionCommandRoute.path.source).toBe(
    "cloud/sessions/:sessionId/commands",
  )
})
