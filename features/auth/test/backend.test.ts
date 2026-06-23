import { expect, test } from "bun:test"

import { authBackendPlugin } from "../src/backend.ts"

test("auth backend plugin exposes auth routes", () => {
  expect(authBackendPlugin.name).toBe("auth")
  expect(authBackendPlugin.routes?.auth.path.source).toBe("/auth")
  expect("events" in authBackendPlugin).toBe(false)
  expect("eventSources" in authBackendPlugin).toBe(false)
})
