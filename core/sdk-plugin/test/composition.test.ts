import { $type, defineIpcSchema } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"

import {
  composeSdkPlugins,
  defineRequest,
  defineSdkPlugin,
  defineSubscription,
  defineUnwrappedSubscription,
} from "../src/index.ts"

const testIpcSchema = defineIpcSchema({
  requests: {
    "inbox.list": {
      response: $type<{ items: string[] }>(),
    },
  },
  streams: {
    "inbox.item": $type<{ id: string }>(),
    "session.message": $type<{ id: string; message: string }>(),
  },
})

describe("SDK plugin composition", () => {
  test("merges plugins that contribute different methods to the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipc: testIpcSchema,
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
      ipc: testIpcSchema,
      create() {
        return {
          inbox: {
            update: () => "update",
          },
        }
      },
    })

    const namespaces = composeSdkPlugins([first, second]).create({ client: {} as never })

    expect(Object.keys(namespaces.inbox)).toEqual(["list", "update"])
  })

  test("rejects duplicate methods in the same namespace", () => {
    const first = defineSdkPlugin({
      name: "first",
      ipc: testIpcSchema,
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
      ipc: testIpcSchema,
      create() {
        return {
          inbox: {
            list: () => "second",
          },
        }
      },
    })

    expect(() => composeSdkPlugins([first, second]).create({ client: {} as never })).toThrow(
      "Duplicate SDK namespace method: inbox.list",
    )
  })

  test("defines typed request and subscription helpers", async () => {
    const sent: unknown[] = []
    const subscribed: unknown[] = []
    const client = {
      send: async (...args: unknown[]) => {
        sent.push(args)
        return { items: ["inb_1"] }
      },
      subscribe: async (...args: unknown[]) => {
        subscribed.push(args)
        return () => {}
      },
    } as never

    const list = defineRequest(client, "inbox.list")
    const subscribe = defineSubscription(client, "inbox.item")

    await expect(list()).resolves.toEqual({ items: ["inb_1"] })
    void subscribe(() => {})

    expect(sent).toEqual([["inbox.list"]])
    expect((subscribed[0] as unknown[])[0]).toBe("inbox.item")
  })

  test("defines unwrapped subscriptions for stream envelopes", () => {
    const messages: string[] = []
    const client = {
      subscribe: async (_target: unknown, onMessage: (payload: unknown) => void) => {
        onMessage({ id: "ses_1", message: "hello" })
        return () => {}
      },
    } as never

    const subscribe = defineUnwrappedSubscription(
      client,
      "session.message",
      ({ message }) => message,
    )

    void subscribe(undefined, (message) => {
      messages.push(message)
    })

    expect(messages).toEqual(["hello"])
  })
})
