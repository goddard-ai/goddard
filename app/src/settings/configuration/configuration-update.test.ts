import { describe, expect, test } from "vitest"

import { deriveUserConfigUpdate } from "./configuration-update.ts"

describe("deriveUserConfigUpdate", () => {
  test("updates one existing nested field", () => {
    expect(
      deriveUserConfigUpdate(
        { daemon: { port: 49_827, browserAccess: { allowedOrigins: ["https://app.test"] } } },
        { daemon: { port: 51_999, browserAccess: { allowedOrigins: ["https://app.test"] } } },
      ),
    ).toEqual({
      operation: "set",
      path: "/daemon/port",
      value: 51_999,
    })
  })

  test("sets an absent parent as one field update", () => {
    expect(deriveUserConfigUpdate({}, { daemon: { port: 51_999 } })).toEqual({
      operation: "set",
      path: "/daemon",
      value: { port: 51_999 },
    })
  })

  test("sets a changed array as one collection update", () => {
    expect(
      deriveUserConfigUpdate(
        { daemon: { browserAccess: { allowedOrigins: ["https://one.test"] } } },
        {
          daemon: {
            browserAccess: {
              allowedOrigins: ["https://one.test", "https://two.test"],
            },
          },
        },
      ),
    ).toEqual({
      operation: "set",
      path: "/daemon/browserAccess/allowedOrigins",
      value: ["https://one.test", "https://two.test"],
    })
  })

  test("collapses a map key rename to the owning map", () => {
    expect(
      deriveUserConfigUpdate(
        { session: { env: { OLD_KEY: "value" } } },
        { session: { env: { NEW_KEY: "value" } } },
      ),
    ).toEqual({
      operation: "set",
      path: "/session/env",
      value: { NEW_KEY: "value" },
    })
  })

  test("removes an optional field", () => {
    expect(deriveUserConfigUpdate({ agents: { default: "pi-acp" } }, {})).toEqual({
      operation: "remove",
      path: "/agents",
    })
  })

  test("does not derive a whole-document replacement", () => {
    expect(
      deriveUserConfigUpdate(
        {},
        {
          agents: { default: "pi-acp" },
          daemon: { port: 51_999 },
        },
      ),
    ).toBeNull()
  })

  test("encodes map keys as JSON Pointer segments", () => {
    expect(
      deriveUserConfigUpdate(
        { session: { env: { "KEY/ONE": "old" } } },
        { session: { env: { "KEY/ONE": "new" } } },
      ),
    ).toEqual({
      operation: "set",
      path: "/session/env/KEY~1ONE",
      value: "new",
    })
  })
})
