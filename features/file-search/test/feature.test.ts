import { describe, expect, test } from "bun:test"

import { fileSearchIpcRoutes } from "../src/daemon-ipc.ts"
import { fileSearchPlugin } from "../src/daemon.ts"
import { FileSearchComposerEntriesRequest } from "../src/schema.ts"
import { fileSearchSdkPlugin } from "../src/sdk.ts"

describe("file-search feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(fileSearchPlugin.name).toBe("file-search")
    expect(Object.keys(fileSearchIpcRoutes)).toEqual(["fileSearch"])
    expect(fileSearchSdkPlugin.ipcRoutes).toBe(fileSearchIpcRoutes)
    expect(
      FileSearchComposerEntriesRequest.parse({
        cwd: "/tmp/project",
        query: "src",
        limit: 10,
      }),
    ).toEqual({
      cwd: "/tmp/project",
      query: "src",
      limit: 10,
    })
  })
})
