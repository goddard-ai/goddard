import { describe, expect, test } from "bun:test"

import { reviewSessionIpcRoutes } from "../src/daemon-ipc.ts"
import { reviewSessionPlugin } from "../src/daemon.ts"
import { ReviewSessionLaunchParams } from "../src/schema.ts"
import { reviewSessionSdkPlugin } from "../src/sdk.ts"

describe("review-session feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(reviewSessionPlugin.name).toBe("review-session")
    expect(reviewSessionIpcRoutes).toHaveProperty("reviewSession")
    expect(reviewSessionSdkPlugin.name).toBe("review-session")
    expect(ReviewSessionLaunchParams.parse({ enabled: true })).toEqual({ enabled: true })
  })
})
