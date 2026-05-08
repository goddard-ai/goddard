import { describe, expect, test } from "bun:test"
import { z } from "zod"

import { composeDaemonPlugins, defineDaemonPlugin } from "../src/index.ts"

describe("daemon plugin composition", () => {
  test("orders plugins by consumed dependencies and composes IPC/config fragments", () => {
    const session = defineDaemonPlugin({
      name: "session",
      provides: {
        session: {
          start: () => "started",
        },
      },
      config: {
        schema: z.object({
          enabled: z.boolean(),
        }),
        scopes: ["user", "project"],
      },
      ipc: {
        requests: {
          "session.create": {
            response: {} as { __unchecked__: { id: string } },
          },
        },
        streams: {},
      },
    })
    const inbox = defineDaemonPlugin({
      name: "inbox",
      consumes: [session],
    })

    const composition = composeDaemonPlugins([inbox, session])

    expect(composition.plugins.map((plugin) => plugin.name)).toEqual(["session", "inbox"])
    expect(Object.keys(composition.ipc.requests)).toEqual(["session.create"])
    expect(composition.config.session.scopes).toEqual(["user", "project"])
  })

  test("rejects duplicate plugin names", () => {
    const first = defineDaemonPlugin({ name: "session" })
    const second = defineDaemonPlugin({ name: "session" })

    expect(() => composeDaemonPlugins([first, second])).toThrow("Duplicate daemon plugin: session")
  })

  test("rejects consumed plugins that are not part of the composition", () => {
    const session = defineDaemonPlugin({ name: "session" })
    const inbox = defineDaemonPlugin({
      name: "inbox",
      consumes: [session],
    })

    expect(() => composeDaemonPlugins([inbox])).toThrow(
      "Daemon plugin inbox consumes session, but session is not composed.",
    )
  })

  test("rejects circular feature dependencies", () => {
    const first = defineDaemonPlugin({
      name: "first",
      get consumes() {
        return [second]
      },
    })
    const second = defineDaemonPlugin({
      name: "second",
      consumes: [first],
    })

    expect(() => composeDaemonPlugins([first, second])).toThrow(
      "Circular daemon plugin dependency: first -> second -> first",
    )
  })
})
