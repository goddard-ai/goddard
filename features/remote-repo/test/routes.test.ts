import { expect, test } from "bun:test"

import { remoteRepoBackendRoutes } from "../src/backend.ts"

test("backend routes keep their logical remote-repo resource grouping", () => {
  expect(remoteRepoBackendRoutes.remoteRepo.path.source).toBe("/remote-repo")
  expect(remoteRepoBackendRoutes.remoteRepo.children.stream.path?.source).toBe("/stream")
})
