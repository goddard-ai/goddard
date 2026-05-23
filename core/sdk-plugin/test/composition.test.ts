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

describe("SDK plugin composition", () => {
  test("merges plugins that contribute different methods to the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      create() {
        return {
          inbox: {
            list: () => "list",
          },
        }
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: testIpcRoutes,
      create() {
        return {
          inbox: {
            update: () => "update",
          },
        }
      },
    })

    const namespaces = composeSdkPlugins([first, second]).create({ client: {} })

    expect(Object.keys(namespaces.inbox)).toEqual(["list", "update"])
  })

  test("rejects duplicate methods in the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipcRoutes: testIpcRoutes,
      create() {
        return {
          inbox: {
            list: () => "first",
          },
        }
      },
    })
    const second = defineSdkPlugin({
      name: "second",
      ipcRoutes: testIpcRoutes,
      create() {
        return {
          inbox: {
            list: () => "second",
          },
        }
      },
    })

    expect(() => composeSdkPlugins([first, second]).create({ client: {} })).toThrow(
      "Duplicate SDK namespace method: inbox.list",
    )
  })
})
