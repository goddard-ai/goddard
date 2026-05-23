import { $type, http, ndjson } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"
import { z } from "zod"

import { composePlugins, definePlugin } from "../src/index.ts"

describe("daemon plugin composition", () => {
  test("orders plugins by consumed dependencies and composes IPC/config fragments", () => {
    const session = definePlugin({
      name: "session",
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
      setup() {
        return {
          provides: {
            session: {
              start: () => "started",
            },
          },
          requestHandlers: {
            "session.create": () => ({ id: "session-1" }),
          },
        }
      },
    })
    const inbox = definePlugin({
      name: "inbox",
      consumes: [session],
    })

    const composition = composePlugins([inbox, session])

    expect(composition.plugins.map((plugin) => plugin.name)).toEqual(["session", "inbox"])
    expect(Object.keys(composition.ipc.requests)).toEqual(["session.create"])
    expect(composition.config.session.scopes).toEqual(["user", "project"])
  })

  test("composes IPC route tree fragments", () => {
    const session = definePlugin({
      name: "session",
      ipcRoutes: {
        session: http.resource("session", {
          create: http.post("create", {
            response: $type<{ id: string }>(),
          }),
        }),
      },
    })
    const inbox = definePlugin({
      name: "inbox",
      ipcRoutes: {
        session: http.resource("session", {
          events: http.get("events", {
            response: ndjson.$type<{ id: string }>(),
          }),
        }),
      },
    })

    const composition = composePlugins([inbox, session])

    expect(Object.keys(composition.ipcRoutes.session.children).sort()).toEqual(["create", "events"])
  })

  test("rejects duplicate plugin names", () => {
    const first = definePlugin({ name: "session" })
    const second = definePlugin({ name: "session" })

    expect(() => composePlugins([first, second])).toThrow("Duplicate daemon plugin: session")
  })

  test("rejects consumed plugins that are not part of the composition", () => {
    const session = definePlugin({ name: "session" })
    const inbox = definePlugin({
      name: "inbox",
      consumes: [session],
    })

    expect(() => composePlugins([inbox])).toThrow(
      "Daemon plugin inbox consumes session, but session is not composed.",
    )
  })

  test("rejects circular feature dependencies", () => {
    const first = definePlugin({
      name: "first",
      get consumes() {
        return [second]
      },
    })
    const second = definePlugin({
      name: "second",
      consumes: [first],
    })

    expect(() => composePlugins([first, second])).toThrow(
      "Circular daemon plugin dependency: first -> second -> first",
    )
  })
})
