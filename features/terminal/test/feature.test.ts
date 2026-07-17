import { describe, expect, test } from "bun:test"

import { terminalIpcRoutes } from "../src/daemon-ipc.ts"
import { terminalPlugin } from "../src/daemon.ts"
import { terminalSdkPlugin } from "../src/sdk.ts"

describe("terminal feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(terminalPlugin.name).toBe("terminal")
    expect(Object.keys(terminalIpcRoutes)).toEqual(["terminal"])
    expect(terminalSdkPlugin.name).toBe("terminal")
  })
})
