import { describe, expect, test } from "bun:test"
import { z } from "zod"

import { $type, defineIpcRoutes, http, listIpcRouteActions, ndjson } from "../src/index.ts"

describe("IPC route tree", () => {
  test("lists action leaves with request and stream metadata", () => {
    const routes = defineIpcRoutes({
      daemon: http.resource("daemon", {
        health: http.get("health", {
          response: $type<{ ok: true }>(),
        }),
      }),
      session: http.resource("session", {
        get: http.post("get", {
          body: z.object({ id: z.string() }),
          response: $type<{ id: string }>(),
        }),
        streamMessages: http.get("stream-messages", {
          query: z.object({ id: z.string() }),
          response: ndjson.$type<{ message: string }>(),
        }),
        launchWorktree: http.resource("launch-worktree", {
          prepare: http.post("prepare", {
            body: z.object({ cwd: z.string() }),
            response: $type<{ path: string }>(),
          }),
        }),
      }),
    })

    const actions = listIpcRouteActions(routes)

    expect(
      actions.map((action) => ({
        keyPath: action.keyPath,
        commandPath: action.commandPath,
        httpPath: action.httpPath,
        requestInput: action.requestInput,
        streamsNdjson: action.streamsNdjson,
      })),
    ).toEqual([
      {
        keyPath: ["daemon", "health"],
        commandPath: ["daemon", "health"],
        httpPath: ["daemon", "health"],
        requestInput: null,
        streamsNdjson: false,
      },
      {
        keyPath: ["session", "get"],
        commandPath: ["session", "get"],
        httpPath: ["session", "get"],
        requestInput: "body",
        streamsNdjson: false,
      },
      {
        keyPath: ["session", "streamMessages"],
        commandPath: ["session", "streamMessages"],
        httpPath: ["session", "stream-messages"],
        requestInput: "query",
        streamsNdjson: true,
      },
      {
        keyPath: ["session", "launchWorktree", "prepare"],
        commandPath: ["session", "launchWorktree", "prepare"],
        httpPath: ["session", "launch-worktree", "prepare"],
        requestInput: "body",
        streamsNdjson: false,
      },
    ])
    expect(actions[1]?.action).toBe(routes.session.children.get)
  })
})
