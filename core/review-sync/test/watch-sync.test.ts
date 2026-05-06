import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { watchReviewSession, type ReviewSyncResult } from "../src/index.ts"
import {
  cleanupReviewSyncFixtures,
  createDeferred,
  createStartedFixture,
  runGit,
  runWatchUntilNextSync,
  sleep,
  writeText,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("watch syncs when the review worktree changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("watch syncs when the agent worktree changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
})

test("watch syncs when the agent branch HEAD changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await runGit(fixture.agentDir, ["commit", "--allow-empty", "-m", "empty"])
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
})

test("watch waits for the expected agent checkout before syncing", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch") {
        started.resolve()
      }
      if (result.command === "sync" && result.status === "ok") {
        controller.abort()
      }
    },
  })

  try {
    await started.promise
    await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])
    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
    await sleep(250)

    expect(results.some((result) => result.command === "sync")).toBe(false)
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("base\n")

    await runGit(fixture.agentDir, ["checkout", "codex/review-sync-test"])
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  } finally {
    clearTimeout(timeout)
  }
})
