import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { watchReviewSession, type ReviewSyncResult } from "../src/index.ts"
import {
  cleanupReviewSyncFixtures,
  createDeferred,
  createStartedFixture,
  currentBranch,
  refExists,
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
  await runGit(fixture.reviewDir, ["checkout", "main"])
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
  expect(await refExists(fixture.agentDir, "refs/heads/review-sync/codex/review-sync-test")).toBe(
    false,
  )
})

test("watch syncs review commits when the agent branch is already checked out", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "main"])

  expect(await currentBranch(fixture.agentDir)).toBe("codex/review-sync-test")

  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "human commit\n")
    await runGit(fixture.reviewDir, ["add", "shared.txt"])
    await runGit(fixture.reviewDir, ["commit", "-m", "human review commit"])
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human commit\n")
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
  expect(await refExists(fixture.agentDir, "refs/heads/review-sync/codex/review-sync-test")).toBe(
    false,
  )
})

test("watch syncs a review commit that records already-rendered content", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "main"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  const firstSync = createDeferred<ReviewSyncResult>()
  const secondSync = createDeferred<ReviewSyncResult>()
  let startedResolved = false
  let firstSyncResolved = false
  let secondSyncResolved = false
  let syncCount = 0
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch") {
        startedResolved = true
        started.resolve()
      }
      if (result.command === "sync" && result.status === "ok") {
        syncCount += 1
        if (syncCount === 1) {
          firstSyncResolved = true
          firstSync.resolve(result)
        }
        if (syncCount === 2) {
          secondSyncResolved = true
          secondSync.resolve(result)
        }
      }
    },
  })

  try {
    await Promise.race([
      started.promise,
      watch.then((result) => {
        if (!startedResolved) {
          throw new Error(`watch stopped before starting: ${result.message}`)
        }
      }),
    ])
    await sleep(100)

    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit before commit\n")

    const firstResult = await Promise.race([
      firstSync.promise,
      watch.then((result) => {
        if (!firstSyncResolved) {
          throw new Error(`watch stopped before first sync: ${result.message}`)
        }
        return firstSync.promise
      }),
    ])
    expect(firstResult.acceptedPatchPath).toBeTruthy()
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe(
      "human edit before commit\n",
    )
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe(
      "human edit before commit\n",
    )

    expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe(
      "M  shared.txt\n",
    )
    const beforeCommitHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()
    await runGit(fixture.reviewDir, ["commit", "-m", "human review commit after sync"])
    const afterCommitHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()

    expect(afterCommitHead).not.toBe(beforeCommitHead)
    await Promise.race([
      secondSync.promise,
      watch.then((result) => {
        if (!secondSyncResolved) {
          throw new Error(`watch stopped before second sync: ${result.message}`)
        }
        return secondSync.promise
      }),
    ])
    expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
      afterCommitHead,
    )
    expect((await runGit(fixture.reviewDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
      "human review commit after sync",
    )
    expect((await runGit(fixture.agentDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
      afterCommitHead,
    )
    expect((await runGit(fixture.agentDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
      "human review commit after sync",
    )

    controller.abort()
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe(
      "human edit before commit\n",
    )
    expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
    expect((await runGit(fixture.agentDir, ["status", "--porcelain=v1"])).stdout).toBe("")
  } finally {
    if (!controller.signal.aborted) {
      controller.abort()
    }
    await watch.catch(() => {})
    clearTimeout(timeout)
  }
})

test("watch preserves a cherry-picked review commit after accepting its patch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const sourceDir = join(fixture.rootDir, "source")
  await runGit(fixture.agentDir, ["worktree", "add", "-b", "human/source", sourceDir, "main"])
  await writeText(join(sourceDir, "shared.txt"), "picked edit\n")
  await runGit(sourceDir, ["add", "shared.txt"])
  await runGit(sourceDir, ["commit", "-m", "picked review edit"])
  const pickedCommit = (await runGit(sourceDir, ["rev-parse", "HEAD"])).stdout.trim()
  await runGit(fixture.reviewDir, ["checkout", "main"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  const sync = createDeferred<ReviewSyncResult>()
  let startedResolved = false
  let syncResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch") {
        startedResolved = true
        started.resolve()
      }
      if (result.command === "sync" && result.status === "ok") {
        syncResolved = true
        sync.resolve(result)
      }
    },
  })

  try {
    await Promise.race([
      started.promise,
      watch.then((result) => {
        if (!startedResolved) {
          throw new Error(`watch stopped before starting: ${result.message}`)
        }
      }),
    ])
    await sleep(100)
    await runGit(fixture.reviewDir, ["cherry-pick", pickedCommit])
    const cherryPickedHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()

    const result = await Promise.race([
      sync.promise,
      watch.then((stopped) => {
        if (!syncResolved) {
          throw new Error(`watch stopped before sync: ${stopped.message}`)
        }
        return sync.promise
      }),
    ])

    expect(result.acceptedPatchPath).toBeTruthy()
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("picked edit\n")
    expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
      cherryPickedHead,
    )
    expect((await runGit(fixture.reviewDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
      "picked review edit",
    )
    expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")

    controller.abort()
    const stopped = await watch
    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
  } finally {
    if (!controller.signal.aborted) {
      controller.abort()
    }
    await watch.catch(() => {})
    clearTimeout(timeout)
  }
})

test("watch syncs when the agent worktree changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "main"])
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  })

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("base\n")
})

test("watch syncs when the agent branch HEAD changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "main"])
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
  await runGit(fixture.reviewDir, ["checkout", "main"])
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
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("base\n")
  } finally {
    clearTimeout(timeout)
  }
})
