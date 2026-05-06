import { describe, expect, test } from "bun:test"

import { terminalAppPlugin } from "../src/app.tsx"
import { terminalIpcRoutes } from "../src/daemon-ipc.ts"
import { terminalPlugin } from "../src/daemon.ts"
import { terminalIdSchema } from "../src/schema.ts"
import { terminalSdkPlugin } from "../src/sdk.ts"

describe("terminal feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(terminalAppPlugin.name).toBe("terminal")
    expect(terminalPlugin.name).toBe("terminal")
    expect(Object.keys(terminalIpcRoutes)).toEqual(["terminal"])
    expect(terminalSdkPlugin.name).toBe("terminal")
    expect(terminalIdSchema.parse("terminal")).toBe("terminal")
  })
})
