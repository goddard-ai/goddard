import { describe, expect, test } from "bun:test"
import { z } from "zod"

import { $type, composeIpcSchemas, defineIpcSchema } from "../src/index.ts"

describe("IPC schema composition", () => {
  test("composes request and stream schema fragments", () => {
    const sessionIpc = defineIpcSchema({
      requests: {
        "session.create": {
          payload: z.object({ id: z.string() }),
          response: $type<{ id: string }>(),
        },
      },
      streams: {
        "session.message": {
          payload: $type<{ id: string; message: string }>(),
          filter: z.object({ id: z.string() }),
        },
      },
    })
    const inboxIpc = defineIpcSchema({
      requests: {
        "inbox.list": {
          response: $type<{ items: unknown[] }>(),
        },
      },
      streams: {},
    })

    const composed = composeIpcSchemas([sessionIpc, inboxIpc])

    expect(Object.keys(composed.requests)).toEqual(["session.create", "inbox.list"])
    expect(Object.keys(composed.streams)).toEqual(["session.message"])
  })

  test("rejects duplicate request names", () => {
    const first = defineIpcSchema({
      requests: {
        "session.create": {
          response: $type<{ ok: true }>(),
        },
      },
      streams: {},
    })
    const second = defineIpcSchema({
      requests: {
        "session.create": {
          response: $type<{ ok: true }>(),
        },
      },
      streams: {},
    })

    expect(() => composeIpcSchemas([first, second])).toThrow(
      "Duplicate IPC request: session.create",
    )
  })

  test("rejects duplicate stream names", () => {
    const first = defineIpcSchema({
      requests: {},
      streams: {
        "session.message": $type<{ id: string }>(),
      },
    })
    const second = defineIpcSchema({
      requests: {},
      streams: {
        "session.message": $type<{ id: string }>(),
      },
    })

    expect(() => composeIpcSchemas([first, second])).toThrow(
      "Duplicate IPC stream: session.message",
    )
  })
})
