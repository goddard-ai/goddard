import { expect, test } from "bun:test"

import { DaemonConfig } from "../src/config.ts"

test("DaemonConfig accepts a global daemon port override", () => {
  const config = DaemonConfig.parse({
    port: 49828,
  })

  expect(config.port).toBe(49828)
})
