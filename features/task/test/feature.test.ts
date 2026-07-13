import { describe, expect, test } from "bun:test"

import { taskIpcRoutes } from "../src/daemon-ipc.ts"
import { taskPlugin } from "../src/daemon.ts"
import { TaskId } from "../src/schema.ts"
import { taskSdkPlugin } from "../src/sdk.ts"

describe("task feature package", () => {
  test("exports the supported task surface", () => {
    expect(taskPlugin.name).toBe("task")
    expect(Object.keys(taskIpcRoutes.task.children)).toEqual([
      "create",
      "get",
      "list",
      "update",
      "setStatus",
      "claim",
      "release",
      "addNote",
      "addLink",
      "removeLink",
    ])
    expect(taskSdkPlugin.name).toBe("task")
    expect(TaskId.parse("tsk_test")).toBe("tsk_test")
  })
})
