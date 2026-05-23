import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"

import { composeSdkPlugins, defineSdkPlugin } from "../src/index.ts"

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
    const plugin = defineSdkPlugin({
      name: "route-only",
      ipcRoutes: testIpcRoutes,
    })

    expect(composeSdkPlugins([plugin]).wrap({ client: {} })).toEqual({})
  })
})
