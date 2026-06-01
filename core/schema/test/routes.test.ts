import { expect, test } from "bun:test"

import { repositories } from "../src/backend/routes.ts"

test("backend routes keep their logical resource grouping", () => {
  expect(repositories.path.source).toBe("/repositories")
  expect(repositories.children.stream.path?.source).toBe("/stream")
})
