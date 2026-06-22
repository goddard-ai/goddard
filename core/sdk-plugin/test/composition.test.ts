import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"

import { composeSdkPlugins, defineSdkPlugin, type InferSdkEvents } from "../src/index.ts"

const testIpcRoutes = defineIpcRoutes({
  inbox: http.resource("inbox", {
    list: http.post("list", {
      response: $type<{ items: string[] }>(),
    }),
  }),
})

const moreTestIpcRoutes = defineIpcRoutes({
  adapter: http.resource("adapter", {
    list: http.post("list", {
      response: $type<{ items: string[] }>(),
    }),
  }),
})

describe("SDK plugin composition", () => {
  test("merges plugins that contribute different methods to the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      wrap() {
        return {
          inbox: {
            list: () => "list",
          },
        }
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: moreTestIpcRoutes,
      wrap() {
        return {
          inbox: {
            update: () => "update",
          },
        }
      },
    })

    const namespaces = composeSdkPlugins([first, second]).wrap({ client: {} })

    expect(Object.keys(namespaces.inbox)).toEqual(["list", "update"])
  })

  test("preserves the raw plugin tuple for composition-time inference", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      wrap() {
        return {
          inbox: {
            list: () => "list",
          },
        }
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: moreTestIpcRoutes,
    })

    const composition = composeSdkPlugins([first, second])

    expect(composition.plugins).toEqual([first, second])
  })

  test("rejects duplicate methods in the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      wrap() {
        return {
          inbox: {
            list: () => "first",
          },
        }
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: moreTestIpcRoutes,
      wrap() {
        return {
          inbox: {
            list: () => "second",
          },
        }
      },
    })

    expect(() => composeSdkPlugins([first, second]).wrap({ client: {} })).toThrow(
      "Duplicate SDK namespace method: inbox.list",
    )
  })

  test("composes plugin route trees for the SDK client runtime", () => {
    const plugin = defineSdkPlugin({
      name: "routes",
      ipcRoutes: testIpcRoutes,
    })

    expect(Object.keys(composeSdkPlugins([plugin]).ipcRoutes)).toEqual(["inbox"])
  })

  test("allows route-only plugins without custom wrappers", () => {
    const list = () => ({ items: [] })
    const plugin = defineSdkPlugin({
      name: "route-only",
      ipcRoutes: testIpcRoutes,
    })

    expect(
      composeSdkPlugins([plugin]).wrap({
        client: {
          inbox: { list },
        },
      }),
    ).toEqual({
      inbox: { list },
    })
  })

  test("composes plugin event definitions for SDK event inference", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      events: {
        "inbox.item.updated": {},
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: moreTestIpcRoutes,
      events: {
        "session.message": {},
      },
    })

    const composition = composeSdkPlugins([first, second])
    type Events = InferSdkEvents<typeof composition>
    const eventName: keyof Events = "session.message"

    expect(eventName).toBe("session.message")
    expect(Object.keys(composition.events)).toEqual(["inbox.item.updated", "session.message"])
  })

  test("rejects duplicate event definitions", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      events: {
        "session.message": {},
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: moreTestIpcRoutes,
      events: {
        "session.message": {},
      },
    })

    expect(() => composeSdkPlugins([first, second])).toThrow("Duplicate SDK event: session.message")
  })
})
