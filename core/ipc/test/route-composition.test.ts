import { describe, expect, test } from "bun:test"

import { $type, composeIpcRoutes, defineIpcRoutes, http, ndjson } from "../src/index.ts"

describe("IPC route composition", () => {
  test("composes Rouzer route fragments", () => {
    const sessionRoutes = defineIpcRoutes({
      session: http.resource("session", {
        create: http.post("create", {
          response: $type<{ id: string }>(),
        }),
      }),
    })
    const inboxRoutes = defineIpcRoutes({
      inbox: http.resource("inbox", {
        events: http.get("events", {
          response: ndjson.$type<{ id: string }>(),
        }),
      }),
    })

    const composed = composeIpcRoutes([sessionRoutes, inboxRoutes])

    expect(Object.keys(composed)).toEqual(["session", "inbox"])
  })

  test("merges matching resource namespaces", () => {
    const createRoutes = defineIpcRoutes({
      session: http.resource("session", {
        create: http.post("create", {
          response: $type<{ id: string }>(),
        }),
      }),
    })
    const eventRoutes = defineIpcRoutes({
      session: http.resource("session", {
        events: http.get("events", {
          response: ndjson.$type<{ id: string }>(),
        }),
      }),
    })

    const composed = composeIpcRoutes([createRoutes, eventRoutes])

    expect(Object.keys(composed.session.children)).toEqual(["create", "events"])
  })

  test("rejects duplicate action ownership", () => {
    const first = defineIpcRoutes({
      session: http.resource("session", {
        create: http.post("create", {
          response: $type<{ id: string }>(),
        }),
      }),
    })
    const second = defineIpcRoutes({
      session: http.resource("session", {
        create: http.post("create", {
          response: $type<{ id: string }>(),
        }),
      }),
    })

    expect(() => composeIpcRoutes([first, second])).toThrow("Duplicate IPC route: session.create")
  })

  test("rejects conflicting resource paths", () => {
    const first = defineIpcRoutes({
      session: http.resource("session", {}),
    })
    const second = defineIpcRoutes({
      session: http.resource("sessions", {}),
    })

    expect(() => composeIpcRoutes([first, second])).toThrow(
      "Conflicting IPC resource path: session",
    )
  })
})
