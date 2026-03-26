import { describe, expect, it } from "vitest"
import { TASK_PRIORITIES, type TaskPlugin } from "../src/tasks.ts"

describe("task plugin contracts", () => {
  it("exports the normalized task priorities", () => {
    expect(TASK_PRIORITIES).toEqual(["urgent", "high", "medium", "low"])
  })

  it("supports provider-specific metadata and extra input fields", async () => {
    const plugin: TaskPlugin<
      { sourceId: string },
      { source: "webhook" },
      { revision: number },
      { reason?: string }
    > = {
      name: "test",
      async addTask(input) {
        return {
          id: input.metadata?.sourceId ?? "new-task",
          title: input.title,
          description: input.description,
          owner: input.owner,
          repo: input.repo,
          priority: input.priority,
          labels: input.labels,
          metadata: input.metadata,
          closed: false,
        }
      },
      async updateTask(input) {
        return {
          id: input.id,
          title: input.title ?? "updated task",
          description: input.description,
          owner: input.owner,
          repo: input.repo,
          priority: input.priority,
          labels: input.labels,
          metadata: input.metadata,
          closed: input.closed ?? false,
        }
      },
      async closeTask(input) {
        return {
          id: input.id,
          title: "closed task",
          metadata: {
            sourceId: input.id,
          },
          closed: true,
        }
      },
    }

    const created = await plugin.addTask({
      title: "Sync webhook task",
      owner: "goddard-ai",
      repo: "goddard",
      metadata: { sourceId: "linear-123" },
      extra: { source: "webhook" },
    })

    const updated = await plugin.updateTask({
      id: created.id,
      title: "Sync webhook task (updated)",
      extra: { revision: 2 },
    })

    const closed = await plugin.closeTask({
      id: updated.id,
      extra: { reason: "done" },
    })

    expect(created.metadata?.sourceId).toBe("linear-123")
    expect(updated.title).toBe("Sync webhook task (updated)")
    expect(closed.closed).toBe(true)
  })
})
