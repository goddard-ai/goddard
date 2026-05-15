import { describe, expect, test } from "bun:test"

import { composeSdkPlugins, defineSdkPlugin } from "../src/index.ts"

describe("SDK plugin composition", () => {
  test("merges plugins that contribute different methods to the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
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
      create() {
        return {
          inbox: {
            update: () => "update",
          },
        }
      },
    })

    const namespaces = composeSdkPlugins([first, second]).create({})

    expect(Object.keys(namespaces.inbox)).toEqual(["list", "update"])
  })

  test("rejects duplicate methods in the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
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
      create() {
        return {
          inbox: {
            list: () => "second",
          },
        }
      },
    })

    expect(() => composeSdkPlugins([first, second]).create({})).toThrow(
      "Duplicate SDK namespace method: inbox.list",
    )
  })
})
