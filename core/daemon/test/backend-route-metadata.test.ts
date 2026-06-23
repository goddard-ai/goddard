import type { HttpRouteTree } from "@goddard-ai/backend-plugin"
import { describe, expect, test } from "bun:test"

import { backendRoutes } from "../src/backend.ts"

describe("backend route metadata", () => {
  test("production resources and actions have descriptions", () => {
    expect(listRoutesMissingDescription(backendRoutes)).toEqual([])
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
