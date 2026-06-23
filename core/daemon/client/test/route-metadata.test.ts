import type { HttpRouteTree } from "@goddard-ai/ipc"
import { describe, expect, test } from "bun:test"

import { daemonIpcRoutes } from "../src/daemon-ipc.ts"

describe("daemon IPC route metadata", () => {
  test("production resources and actions have descriptions", () => {
    expect(listRoutesMissingDescription(daemonIpcRoutes)).toEqual([])
  })
})

function listRoutesMissingDescription(routes: HttpRouteTree) {
  const missing: string[] = []
  collectRoutesMissingDescription(routes, missing)
  return missing
}

function collectRoutesMissingDescription(
  routes: HttpRouteTree,
  missing: string[],
  path: readonly string[] = [],
) {
  for (const [key, node] of Object.entries(routes)) {
    const nextPath = [...path, key]
    if (!node.metadata?.description) {
      missing.push(nextPath.join("."))
    }

    if (node.kind === "resource") {
      collectRoutesMissingDescription(node.children, missing, nextPath)
    }
  }
}
