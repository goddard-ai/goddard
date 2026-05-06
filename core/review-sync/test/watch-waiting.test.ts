import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { pauseReviewSession, watchReviewSession, type ReviewSyncResult } from "../src/index.ts"
import {
  cleanupReviewSyncFixtures,
  createDeferred,
  createFixture,
  createStartedFixture,
  currentBranch,
  runGit,
  sleep,
  waitForFileContent,
  writeText,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("watch waits for an agent branch to get checked out before starting", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const waiting = createDeferred<void>()
  const watching = createDeferred<void>()
  let waitingResolved = false
  let watchingResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch" && result.message.startsWith("Waiting for ")) {
        waitingResolved = true
        waiting.resolve()
      }
      if (result.command === "watch" && result.reviewBranch) {
        watchingResolved = true
        watching.resolve()
        controller.abort()
      }
    },
  })

  try {
    await Promise.race([
      waiting.promise,
      watch.then((result) => {
        if (!waitingResolved) {
          throw new Error(`watch stopped before waiting: ${result.message}`)
        }
      }),
    ])
    await sleep(250)

    expect(results.some((result) => result.command === "start")).toBe(false)
    expect(await currentBranch(fixture.agentDir)).toBe("codex/temporary")
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
    expect(results.find((result) => result.message.startsWith("Waiting for "))?.message).toContain(
      "Checked out review-sync/codex/review-sync-test",
    )

    await runGit(fixture.agentDir, ["checkout", "codex/review-sync-test"])
    await Promise.race([
      watching.promise,
      watch.then((result) => {
        if (!watchingResolved) {
          throw new Error(`watch stopped before watching: ${result.message}`)
        }
      }),
    ])
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(results.some((result) => result.command === "start" && result.status === "ok")).toBe(
      true,
    )
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch prepares the review branch from the branch ref while waiting", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])
  await writeText(join(fixture.agentDir, "shared.txt"), "branch ref edit\n")
  await runGit(fixture.agentDir, ["add", "shared.txt"])
  await runGit(fixture.agentDir, ["commit", "-m", "branch ref edit"])
  await runGit(fixture.agentDir, ["update-ref", "refs/heads/codex/review-sync-test", "HEAD"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const waiting = createDeferred<void>()
  let waitingResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.message.startsWith("Waiting for ")) {
        waitingResolved = true
        waiting.resolve()
        controller.abort()
      }
    },
  })

  try {
    await Promise.race([
      waiting.promise,
      watch.then((result) => {
        if (!waitingResolved) {
          throw new Error(`watch stopped before waiting: ${result.message}`)
        }
      }),
    ])
    const stopped = await watch

    expect(stopped.status).toBe("ok")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(await currentBranch(fixture.agentDir)).toBe("codex/temporary")
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("branch ref edit\n")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch does not retry on its own branch-ref preparation events", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const waiting = createDeferred<void>()
  let waitingResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    verbose: true,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch" && result.message.startsWith("Waiting for ")) {
        waitingResolved = true
        waiting.resolve()
      }
    },
  })

  try {
    await Promise.race([
      waiting.promise,
      watch.then((result) => {
        if (!waitingResolved) {
          throw new Error(`watch stopped before waiting: ${result.message}`)
        }
      }),
    ])
    await sleep(500)
    controller.abort()
    const stopped = await watch

    expect(stopped.status).toBe("ok")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(
      results.filter((result) =>
        result.message.includes("prepared review-sync/codex/review-sync-test in"),
      ).length,
    ).toBe(1)
    expect(
      results.some((result) => result.message.includes("repository metadata changed; retrying")),
    ).toBe(false)
  } finally {
    clearTimeout(timeout)
  }
})

test("watch syncs human commits made while waiting for agent checkout", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const waiting = createDeferred<void>()
  const started = createDeferred<ReviewSyncResult>()
  let waitingResolved = false
  let startedResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.message.startsWith("Waiting for ")) {
        waitingResolved = true
        waiting.resolve()
      }
      if (result.command === "start") {
        startedResolved = true
        started.resolve(result)
        controller.abort()
      }
    },
  })

  try {
    await Promise.race([
      waiting.promise,
      watch.then((result) => {
        if (!waitingResolved) {
          throw new Error(`watch stopped before waiting: ${result.message}`)
        }
      }),
    ])

    await writeText(join(fixture.reviewDir, "shared.txt"), "human commit\n")
    await runGit(fixture.reviewDir, ["add", "shared.txt"])
    await runGit(fixture.reviewDir, ["commit", "-m", "human review commit"])
    await runGit(fixture.agentDir, ["checkout", "codex/review-sync-test"])

    const startResult = await Promise.race([
      started.promise,
      watch.then((result) => {
        if (!startedResolved) {
          throw new Error(`watch stopped before start: ${result.message}`)
        }
        return started.promise
      }),
    ])
    const stopped = await watch

    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(startResult.acceptedPatchPath).toBeTruthy()
    expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human commit\n")
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human commit\n")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch leaves a dirty review worktree alone while waiting", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])
  await runGit(fixture.reviewDir, ["checkout", "-B", "human-base"])
  await writeText(join(fixture.reviewDir, "local.txt"), "dirty\n")

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const waiting = createDeferred<ReviewSyncResult>()
  let waitingResolved = false
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch" && result.message.startsWith("Waiting for ")) {
        waitingResolved = true
        waiting.resolve(result)
        controller.abort()
      }
    },
  })

  try {
    const waitingResult = await Promise.race([
      waiting.promise,
      watch.then((result) => {
        if (!waitingResolved) {
          throw new Error(`watch stopped before waiting: ${result.message}`)
        }
        return waiting.promise
      }),
    ])
    const stopped = await watch

    expect(stopped.status).toBe("ok")
    expect(controller.signal.reason).not.toBe(timeoutReason)
    expect(waitingResult.message).toContain("Review worktree has local changes")
    expect(await currentBranch(fixture.reviewDir)).toBe("human-base")
    expect(await readFile(join(fixture.reviewDir, "local.txt"), "utf-8")).toBe("dirty\n")
  } finally {
    clearTimeout(timeout)
  }
})

test("watch refreshes the review worktree from the target branch ref while waiting", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    signal: controller.signal,
    onResult: (result) => {
      if (result.command === "watch") {
        started.resolve()
      }
    },
  })

  try {
    await started.promise
    await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

    const branchWriterDir = join(fixture.rootDir, "branch-writer")
    await runGit(fixture.agentDir, ["worktree", "add", branchWriterDir, "codex/review-sync-test"])
    await writeText(join(branchWriterDir, "shared.txt"), "branch ref edit\n")
    await runGit(branchWriterDir, ["add", "shared.txt"])
    await runGit(branchWriterDir, ["commit", "-m", "branch ref edit"])

    await waitForFileContent(join(fixture.reviewDir, "shared.txt"), "branch ref edit\n")
    expect(await currentBranch(fixture.agentDir)).toBe("codex/temporary")
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
    controller.abort()

    const stopped = await watch
    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
  } finally {
    clearTimeout(timeout)
  }
})

test("watch reuses an existing session when the agent branch is not checked out", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const branchWriterDir = join(fixture.rootDir, "branch-writer")
  await runGit(fixture.agentDir, ["worktree", "add", branchWriterDir, "codex/review-sync-test"])
  await writeText(join(branchWriterDir, "shared.txt"), "branch ref edit\n")
  await runGit(branchWriterDir, ["add", "shared.txt"])
  await runGit(branchWriterDir, ["commit", "-m", "branch ref edit"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const watching = createDeferred<void>()
  let watchingResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch" && result.reviewBranch) {
        watchingResolved = true
        watching.resolve()
      }
    },
  })

  try {
    await Promise.race([
      watching.promise,
      watch.then((result) => {
        if (!watchingResolved) {
          throw new Error(`watch stopped before watching: ${result.message}`)
        }
      }),
    ])
    await waitForFileContent(join(fixture.reviewDir, "shared.txt"), "branch ref edit\n")
    expect(results.some((result) => result.message.startsWith("Waiting for "))).toBe(false)
    expect(await currentBranch(fixture.agentDir)).toBe("codex/temporary")
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")

    controller.abort()
    const stopped = await watch
    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
  } finally {
    clearTimeout(timeout)
  }
})

test("watch refreshes a paused session from the branch ref while agent checkout is unavailable", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  await pauseReviewSession({
    cwd: fixture.reviewDir,
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const branchWriterDir = join(fixture.rootDir, "branch-writer")
  await runGit(fixture.agentDir, ["worktree", "add", branchWriterDir, "codex/review-sync-test"])
  await writeText(join(branchWriterDir, "shared.txt"), "branch ref edit after pause\n")
  await runGit(branchWriterDir, ["add", "shared.txt"])
  await runGit(branchWriterDir, ["commit", "-m", "branch ref edit after pause"])

  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const watching = createDeferred<void>()
  let watchingResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch" && result.reviewBranch) {
        watchingResolved = true
        watching.resolve()
      }
    },
  })

  try {
    await Promise.race([
      watching.promise,
      watch.then((result) => {
        if (!watchingResolved) {
          throw new Error(`watch stopped before watching: ${result.message}`)
        }
      }),
    ])
    await waitForFileContent(join(fixture.reviewDir, "shared.txt"), "branch ref edit after pause\n")
    expect(results.some((result) => result.command === "resume" && result.status === "ok")).toBe(
      true,
    )
    expect(await currentBranch(fixture.agentDir)).toBe("codex/temporary")
    expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")

    controller.abort()
    const stopped = await watch
    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
  } finally {
    clearTimeout(timeout)
  }
})

test("watch warns when branch ref refresh is blocked by human edits", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  const warning = createDeferred<ReviewSyncResult>()
  let warningResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd: fixture.reviewDir,
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch") {
        started.resolve()
      }
      if (result.message.startsWith("Warning: review refresh skipped")) {
        warningResolved = true
        warning.resolve(result)
      }
    },
  })

  try {
    await started.promise
    await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])
    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")

    const branchWriterDir = join(fixture.rootDir, "branch-writer")
    await runGit(fixture.agentDir, ["worktree", "add", branchWriterDir, "codex/review-sync-test"])
    await writeText(join(branchWriterDir, "shared.txt"), "branch ref edit\n")
    await runGit(branchWriterDir, ["add", "shared.txt"])
    await runGit(branchWriterDir, ["commit", "-m", "branch ref edit"])

    const warningResult = await Promise.race([
      warning.promise,
      watch.then((result) => {
        if (!warningResolved) {
          throw new Error(`watch stopped before warning: ${result.message}`)
        }
        return warning.promise
      }),
    ])
    expect(warningResult.message).toContain("has unapplied human edits")
    expect(results.filter((result) => result.message.startsWith("Warning:")).length).toBe(1)
    expect(results.some((result) => result.command === "sync")).toBe(false)
    expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")

    controller.abort()
    const stopped = await watch
    expect(stopped.status).toBe("paused")
    expect(controller.signal.reason).not.toBe(timeoutReason)
  } finally {
    clearTimeout(timeout)
  }
})
