import { $type as $backendType, http as backendHttp } from "@goddard-ai/backend-plugin"
import { $type, http, ndjson } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"
import { z } from "zod"

import {
  composePlugins,
  createDaemonEventBus,
  definePlugin,
  event,
  matchesDaemonEventFilter,
} from "../src/index.ts"

describe("daemon plugin composition", () => {
  test("orders plugins by consumed dependencies and composes route/config fragments", () => {
    const session = definePlugin({
      name: "session",
      config: {
        session: {
          schema: z.object({
            enabled: z.boolean(),
          }),
          scopes: ["user", "project"],
        },
      },
      jsonSchemas: [
        {
          name: "session.json",
          schema: z.object({
            enabled: z.boolean(),
          }),
        },
      ],
      ipcRoutes: {
        session: http.resource("session", {
          create: http.post("create", {
            response: $type<{ id: string }>(),
          }),
        }),
      },
      setup() {
        return {
          provides: {
            session: {
              start: () => "started",
            },
          },
          ipcHandlers: {
            session: {
              create: () => ({ id: "session-1" }),
            },
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
    expect(Object.keys(composition.ipcRoutes.session.children)).toEqual(["create"])
    expect(composition.config.session.scopes).toEqual(["user", "project"])
    expect(composition.jsonSchemas.map((schema) => schema.name)).toEqual(["session.json"])
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

  test("composes backend route tree fragments", () => {
    const auth = definePlugin({
      name: "auth",
      backendRoutes: {
        auth: backendHttp.resource("auth", {
          session: backendHttp.resource("session", {
            current: backendHttp.get("current", {
              response: $backendType<{ token: string }>(),
            }),
          }),
        }),
      },
    })
    const pullRequest = definePlugin({
      name: "pull-request",
      backendRoutes: {
        pullRequests: backendHttp.resource("pull-requests", {
          create: backendHttp.post("create", {
            response: $backendType<{ number: number }>(),
          }),
        }),
      },
    })

    const composition = composePlugins([pullRequest, auth])

    expect(Object.keys(composition.backendRoutes).sort()).toEqual(["auth", "pullRequests"])
  })

  test("rejects duplicate plugin names", () => {
    const first = definePlugin({ name: "session" })
    const second = definePlugin({ name: "session" })

    expect(() => composePlugins([first, second])).toThrow("Duplicate daemon plugin: session")
  })

  test("rejects duplicate JSON schema artifact names", () => {
    const first = definePlugin({
      name: "first",
      jsonSchemas: [{ name: "config.json", schema: z.object({}) }],
    })
    const second = definePlugin({
      name: "second",
      jsonSchemas: [{ name: "config.json", schema: z.object({}) }],
    })

    expect(() => composePlugins([first, second])).toThrow(
      "Duplicate daemon plugin JSON schema artifact: config.json",
    )
  })

  test("rejects duplicate event names", () => {
    const first = definePlugin({
      name: "first",
      events: {
        "session.turn.ended": event<{ sessionId: string }>(),
      },
    })
    const second = definePlugin({
      name: "second",
      events: {
        "session.turn.ended": event<{ sessionId: string }>(),
      },
    })

    expect(() => composePlugins([first, second])).toThrow(
      "Duplicate daemon plugin event: session.turn.ended",
    )
  })

  test("types event access by ownership and consumed dependencies", () => {
    const session = definePlugin({
      name: "session",
      events: {
        "session.turn.ended": event<{ sessionId: string }>({ debug: "session.lifecycle" }),
      },
      setup({ events }) {
        events.emit("session.turn.ended", { sessionId: "session-1" })
        events.on("session.turn.ended", (payload) => payload.sessionId)

        // @ts-expect-error Plugins cannot emit events they do not declare.
        events.emit("pull_request.created", { pullRequestId: "pr_1" })
      },
    })
    const inbox = definePlugin({
      name: "inbox",
      consumes: [session],
      setup({ events }) {
        events.on("session.turn.ended", (payload) => payload.sessionId)

        // @ts-expect-error Plugins cannot emit events declared by consumed plugins.
        events.emit("session.turn.ended", { sessionId: "session-1" })
        // @ts-expect-error Plugins can only listen to self or consumed plugin events.
        events.on("pull_request.created", () => {})
      },
    })

    const composition = composePlugins([inbox, session])

    expect(composition.events["session.turn.ended"]).toBeDefined()
    expect(composition.events["session.turn.ended"]?.log).toEqual({
      debug: "session.lifecycle",
    })
  })

  test("event bus emits observable envelopes and waits for async observers and listeners", async () => {
    const events = createDaemonEventBus({
      "session.stopping": event<{ id: string }>({ debug: "session.lifecycle" }),
    })
    const calls: string[] = []
    const envelopes: Array<{ name: string; payload: unknown; debug?: string }> = []

    events.observe(async (envelope) => {
      await Promise.resolve()
      envelopes.push({
        name: envelope.name,
        payload: envelope.payload,
        debug: envelope.log?.debug,
      })
      calls.push(`observer:${(envelope.payload as { id: string }).id}`)
    })

    events.on("session.stopping", async (payload) => {
      await Promise.resolve()
      calls.push(`listener:${(payload as { id: string }).id}`)
    })

    calls.push("before")
    await events.emit("session.stopping", { id: "session-1" })
    calls.push("after")

    expect(calls).toEqual(["before", "observer:session-1", "listener:session-1", "after"])
    expect(envelopes).toEqual([
      {
        name: "session.stopping",
        payload: { id: "session-1" },
        debug: "session.lifecycle",
      },
    ])
  })

  test("event bus observers can unsubscribe independently of named listeners", async () => {
    const events = createDaemonEventBus()
    const observed: unknown[] = []
    const listened: unknown[] = []

    const unsubscribe = events.observe((envelope) => {
      observed.push(envelope.payload)
    })
    events.on("session.stopping", (payload) => {
      listened.push(payload)
    })

    unsubscribe()
    await events.emit("session.stopping", { id: "session-1" })

    expect(observed).toEqual([])
    expect(listened).toEqual([{ id: "session-1" }])
  })

  test("daemon event filters match names and exact payload paths", () => {
    const envelope = {
      name: "session.activated",
      payload: {
        sessionId: "ses_123",
        worktree: {
          state: "mounted",
          counters: [1, { done: true }],
        },
      },
    }

    expect(
      matchesDaemonEventFilter(envelope, {
        names: ["session.activated"],
        where: [
          { path: "sessionId", equals: "ses_123" },
          { path: "worktree.state", equals: "mounted" },
          { path: "worktree.counters", equals: [1, { done: true }] },
        ],
      }),
    ).toBe(true)
    expect(
      matchesDaemonEventFilter(envelope, {
        names: ["session.launch.failed"],
      }),
    ).toBe(false)
    expect(
      matchesDaemonEventFilter(envelope, {
        where: [{ path: "worktree.missing", equals: "mounted" }],
      }),
    ).toBe(false)
    expect(
      matchesDaemonEventFilter(envelope, {
        where: [{ path: "worktree.state", equals: "prepared" }],
      }),
    ).toBe(false)
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
