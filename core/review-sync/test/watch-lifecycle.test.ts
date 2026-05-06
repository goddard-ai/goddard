import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { pauseReviewSession, statusReviewSession, watchReviewSession } from "../src/index.ts"
import {
  cleanupReviewSyncFixtures,
  cliPath,
  createDeferred,
  createFixture,
  createStartedFixture,
  currentBranch,
  runGit,
  runProcessUntilOutput,
  runWatchUntilNextSync,
  writeText,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("cli watch accepts an agent branch from the review worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcessUntilOutput(
    fixture.reviewDir,
    "bun",
    [cliPath, "watch", "codex/review-sync-test"],
    "Watching review sync",
  )

  expect(result.stdout).toContain("Started review sync")
  expect(result.stdout).toContain("Watching review sync")
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
})

test("cli watch --verbose explains watcher setup", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcessUntilOutput(
    fixture.reviewDir,
    "bun",
    [cliPath, "watch", "--verbose", "codex/review-sync-test"],
    "Verbose: watchers are armed; waiting for changes.",
  )
  const output = `${result.stdout}\n${result.stderr}`

  expect(output).toContain("Verbose: resolving session for codex/review-sync-test.")
  expect(output).toContain("Verbose: watching paths:")
  expect(output).toContain("Watching review sync")
})

test("watch starts a session from an agent branch before syncing", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  const { results, stopped } = await runWatchUntilNextSync(
    fixture.reviewDir,
    async () => {
      await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
    },
    "codex/review-sync-test",
  )

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "start" && result.status === "ok")).toBe(true)
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("watch restarts a paused session before syncing", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const pause = await pauseReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(pause.status).toBe("paused")

  const { results, stopped } = await runWatchUntilNextSync(
    fixture.reviewDir,
    async () => {
      await writeText(join(fixture.agentDir, "shared.txt"), "agent edit after pause\n")
    },
    "codex/review-sync-test",
  )

  expect(stopped.status).toBe("paused")
  expect(results.some((result) => result.command === "start" && result.status === "ok")).toBe(true)
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe(
    "agent edit after pause\n",
  )
})

test("watch can start another session after the previous watch exits", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  const secondAgentDir = join(fixture.rootDir, "agent-two")
  await runGit(fixture.agentDir, ["branch", "codex/second-review-sync-test", "main"])
  await runGit(fixture.agentDir, [
    "worktree",
    "add",
    secondAgentDir,
    "codex/second-review-sync-test",
  ])

  const first = await runWatchUntilNextSync(
    fixture.reviewDir,
    async () => {
      await writeText(join(fixture.reviewDir, "shared.txt"), "first human edit\n")
    },
    "codex/review-sync-test",
  )

  expect(first.stopped.status).toBe("paused")
  expect(first.results.some((result) => result.command === "start" && result.status === "ok")).toBe(
    true,
  )
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("first human edit\n")

  const second = await runWatchUntilNextSync(
    fixture.reviewDir,
    async () => {
      await writeText(join(fixture.reviewDir, "shared.txt"), "second human edit\n")
    },
    "codex/second-review-sync-test",
  )

  expect(second.stopped.status).toBe("paused")
  expect(
    second.results.some((result) => result.command === "start" && result.status === "ok"),
  ).toBe(true)
  expect(second.stopped.reviewBranch).toBe("review-sync/codex/second-review-sync-test")
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("first human edit\n")
  expect(await readFile(join(secondAgentDir, "shared.txt"), "utf-8")).toBe("second human edit\n")
})

test("watch pauses and restores the starting review branch on exit", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "-B", "review-base"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  let startedResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.reviewBranch) {
        startedResolved = true
        started.resolve()
        controller.abort()
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
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(stopped.message).toContain("Paused review sync. Checked out review-base.")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(await currentBranch(fixture.reviewDir)).toBe("review-base")
    expect((await statusReviewSession({ cwd: fixture.reviewDir })).status).toBe("paused")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch explains when restoring the starting review branch fails", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "-B", "review-base"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  let startedResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.reviewBranch) {
        startedResolved = true
        started.resolve()
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
    await runGit(fixture.reviewDir, ["branch", "-D", "review-base"])
    controller.abort()
    const stopped = await watch

    expect(stopped.status).toBe("error")
    expect(stopped.exitCode).toBe(1)
    expect(stopped.message).toContain("Cleanup did not complete")
    expect(stopped.message).toContain("Could not check out review-base")
    expect(stopped.message).toContain("pathspec")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
    expect((await statusReviewSession({ cwd: fixture.reviewDir })).status).toBe("paused")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch checks out the saved review branch before reusing a session", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.reviewDir, ["checkout", "main"])
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.reviewBranch) {
        controller.abort()
      }
    },
  })

  try {
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(stopped.message).toContain("Checked out main.")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(await currentBranch(fixture.reviewDir)).toBe("main")
  } finally {
    clearTimeout(timeout)
  }
})
