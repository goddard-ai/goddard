import * as fs from "node:fs/promises"
import path from "node:path"
import { statusReviewSession } from "@goddard-ai/review-sync"
import { afterEach, describe, expect, test } from "bun:test"

import {
  findRunningSprintSyncs,
  runSprintSync,
  type SprintSyncReport,
  type SprintSyncStopReport,
} from "../src"
import {
  cleanupTestRepos,
  createLinkedWorktree,
  createSprintRepo,
  currentBranch,
  git,
  runCli,
  spawnCli,
  writeSprintLock,
} from "./support"

type SyncOutput = SprintSyncReport & {
  ok: boolean
  sprint: string | null
  agentBranch: string | null
  reviewBranch: string | null
  reviewSync: {
    status: string
    command: string
    reviewBranch?: string
    message: string
  } | null
}

describe("sprint-branch sync", () => {
  afterEach(cleanupTestRepos)

  test("watches review-sync for the resolved sprint review branch", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    const reviewWorktree = await createLinkedWorktree(repo, "main")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "agent review work\n")

    const controller = new AbortController()
    const timeoutReason = "sprint sync test timeout"
    const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
    const watching = createDeferred<void>()
    let watchingResolved = false
    const results: NonNullable<SyncOutput["reviewSync"]>[] = []
    const syncPromise = runSprintSync({
      cwd: reviewWorktree,
      sprint: "example",
      signal: controller.signal,
      onResult: async (result) => {
        results.push(result)
        if (result.command === "watch" && result.reviewBranch) {
          watchingResolved = true
          watching.resolve()
        }
        if (result.command === "sync" && result.status === "ok") {
          controller.abort()
        }
      },
    })

    try {
      await Promise.race([
        watching.promise,
        syncPromise.then((sync) => {
          if (!watchingResolved) {
            throw new Error(`sync stopped before watching: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      expect(await currentBranch(reviewWorktree)).toBe("review-sync/sprint/example/review")

      await fs.writeFile(path.join(repo, "feature.txt"), "agent changed while watched\n")
      const sync = await syncPromise

      expect(controller.signal.reason).not.toBe(timeoutReason)
      expect(sync.ok).toBe(true)
      expect(sync.sprint).toBe("example")
      expect(sync.agentBranch).toBe("sprint/example/review")
      expect(sync.reviewBranch).toBe("review-sync/sprint/example/review")
      expect(sync.reviewSync?.command).toBe("watch")
      expect(sync.reviewSync?.status).toBe("paused")
      expect(results.some((result) => result.command === "start" && result.status === "ok")).toBe(
        true,
      )
      expect(results.some((result) => result.command === "watch" && result.status === "ok")).toBe(
        true,
      )
      expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(
        true,
      )
      expect(await fs.readFile(path.join(reviewWorktree, "feature.txt"), "utf-8")).toBe(
        "agent changed while watched\n",
      )
    } finally {
      clearTimeout(timeout)
    }
  })

  test("refuses non-interactive sync without a strong sprint context", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    const result = await runCli(repo, ["sync", "--json"])
    const sync = JSON.parse(result.stdout) as SyncOutput & {
      diagnostics: Array<{ code: string }>
    }

    expect(result.exitCode).toBe(1)
    expect(sync.ok).toBe(false)
    expect(sync.reviewSync).toBe(null)
    expect(sync.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
      "sprint_selection_required",
    )
  })

  test("waits for a live sprint lock before initial watch", async () => {
    const { repo, reviewWorktree } = await createSprintSyncFixture()
    const lockPath = await writeSprintLock(repo, "example", { command: "start" })
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort("sprint sync test timeout"), 5000)
    const waiting = createDeferred<void>()
    const watching = createDeferred<void>()
    let waitingResolved = false
    let watchingResolved = false
    const syncPromise = runSprintSync({
      cwd: reviewWorktree,
      sprint: "example",
      signal: controller.signal,
      onResult: async (result) => {
        if (result.message.includes("Waiting for sprint branch operation")) {
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
        syncPromise.then((sync) => {
          if (!waitingResolved) {
            throw new Error(`sync stopped before waiting: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      expect(await currentBranch(reviewWorktree)).not.toBe("review-sync/sprint/example/review")

      await fs.rm(lockPath, { force: true })
      await Promise.race([
        watching.promise,
        syncPromise.then((sync) => {
          if (!watchingResolved) {
            throw new Error(`sync stopped before watching: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      const sync = await syncPromise

      expect(sync.ok).toBe(true)
      expect(sync.reviewSync?.command).toBe("watch")
      expect(sync.reviewSync?.status).toBe("paused")
      expect(await fs.readFile(path.join(reviewWorktree, "feature.txt"), "utf-8")).toBe(
        "agent review work\n",
      )
    } finally {
      clearTimeout(timeout)
      controller.abort()
      await fs.rm(lockPath, { force: true })
    }
  })

  test("defers watch-triggered sync while a sprint lock is live", async () => {
    const { repo, reviewWorktree } = await createSprintSyncFixture()
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort("sprint sync test timeout"), 5000)
    const watching = createDeferred<void>()
    const waiting = createDeferred<void>()
    const synced = createDeferred<void>()
    let watchingResolved = false
    let waitingResolved = false
    let syncedResolved = false
    const syncPromise = runSprintSync({
      cwd: reviewWorktree,
      sprint: "example",
      signal: controller.signal,
      onResult: async (result) => {
        if (result.command === "watch" && result.reviewBranch && !watchingResolved) {
          watchingResolved = true
          watching.resolve()
        }
        if (result.message.includes("Waiting for sprint branch operation") && !waitingResolved) {
          waitingResolved = true
          waiting.resolve()
        }
        if (result.command === "sync" && result.status === "ok" && !syncedResolved) {
          syncedResolved = true
          synced.resolve()
          controller.abort()
        }
      },
    })

    let lockPath: string | null = null
    try {
      await Promise.race([
        watching.promise,
        syncPromise.then((sync) => {
          if (!watchingResolved) {
            throw new Error(`sync stopped before watching: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      expect(await fs.readFile(path.join(reviewWorktree, "feature.txt"), "utf-8")).toBe(
        "agent review work\n",
      )

      lockPath = await writeSprintLock(repo, "example", { command: "approve" })
      await fs.writeFile(path.join(repo, "feature.txt"), "agent changed while locked\n")
      await Promise.race([
        waiting.promise,
        syncPromise.then((sync) => {
          if (!waitingResolved) {
            throw new Error(`sync stopped before waiting: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      await Bun.sleep(250)
      expect(await fs.readFile(path.join(reviewWorktree, "feature.txt"), "utf-8")).toBe(
        "agent review work\n",
      )

      await fs.rm(lockPath, { force: true })
      lockPath = null
      await Promise.race([
        synced.promise,
        syncPromise.then((sync) => {
          if (!syncedResolved) {
            throw new Error(`sync stopped before syncing: ${sync.reviewSync?.message}`)
          }
        }),
      ])
      const sync = await syncPromise

      expect(sync.ok).toBe(true)
      expect(await fs.readFile(path.join(reviewWorktree, "feature.txt"), "utf-8")).toBe(
        "agent changed while locked\n",
      )
    } finally {
      clearTimeout(timeout)
      controller.abort()
      if (lockPath) {
        await fs.rm(lockPath, { force: true })
      }
    }
  })

  test("stop-sync stops a sync process started in the same working directory", async () => {
    const { reviewWorktree } = await createSprintSyncFixture()
    const syncProcess = spawnCli(reviewWorktree, ["sync", "--sprint", "example", "--json"])
    let syncExited = false

    try {
      await waitForReviewSyncCheckout(reviewWorktree)
      const result = await runCli(reviewWorktree, ["stop-sync", "--json"])
      const stop = JSON.parse(result.stdout) as SprintSyncStopReport

      expect(result.exitCode).toBe(0)
      expect(stop.ok).toBe(true)
      expect(stop.stopped).toBe(1)

      const exitCode = await waitForCliExit(syncProcess, "sprint-branch sync")
      syncExited = true
      const [stdout, stderr] = await Promise.all([
        new Response(syncProcess.stdout).text(),
        new Response(syncProcess.stderr).text(),
      ])
      const sync = JSON.parse(stdout) as SyncOutput

      expect(exitCode).toBe(0)
      expect(stderr).toBe("")
      expect(sync.ok).toBe(true)
      expect(sync.reviewSync?.command).toBe("watch")
      expect(sync.reviewSync?.status).toBe("paused")
    } finally {
      if (!syncExited) {
        syncProcess.kill("SIGTERM")
        await syncProcess.exited
      }
    }
  })

  test("stop-sync ignores sync processes from a different working directory", async () => {
    const { repo, reviewWorktree } = await createSprintSyncFixture()
    const syncProcess = spawnCli(reviewWorktree, ["sync", "--sprint", "example", "--json"])
    let syncExited = false

    try {
      await waitForReviewSyncCheckout(reviewWorktree)
      const missedResult = await runCli(repo, ["stop-sync", "--json"])
      const missed = JSON.parse(missedResult.stdout) as SprintSyncStopReport

      expect(missedResult.exitCode).toBe(0)
      expect(missed.stopped).toBe(0)
      expect(await exitsWithin(syncProcess, 300)).toBe(false)

      const stopResult = await runCli(reviewWorktree, ["stop-sync", "--json"])
      const stopped = JSON.parse(stopResult.stdout) as SprintSyncStopReport
      expect(stopped.stopped).toBe(1)

      expect(await waitForCliExit(syncProcess, "sprint-branch sync")).toBe(0)
      syncExited = true
    } finally {
      if (!syncExited) {
        syncProcess.kill("SIGTERM")
        await syncProcess.exited
      }
    }
  })

  test("sync refuses to start while another sync is running in the same working directory", async () => {
    const { reviewWorktree } = await createSprintSyncFixture()
    const syncProcess = spawnCli(reviewWorktree, ["sync", "--sprint", "example", "--json"])
    let syncExited = false

    try {
      await waitForReviewSyncCheckout(reviewWorktree)
      const result = await runCli(reviewWorktree, ["sync", "--sprint", "example", "--json"])
      const sync = JSON.parse(result.stdout) as SyncOutput & {
        diagnostics: Array<{ code: string }>
      }

      expect(result.exitCode).toBe(1)
      expect(sync.ok).toBe(false)
      expect(sync.reviewSync).toBe(null)
      expect(sync.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
        "sync_already_running",
      )
      expect(await exitsWithin(syncProcess, 300)).toBe(false)

      const stopResult = await runCli(reviewWorktree, ["stop-sync", "--json"])
      const stopped = JSON.parse(stopResult.stdout) as SprintSyncStopReport
      expect(stopped.stopped).toBe(1)

      expect(await waitForCliExit(syncProcess, "sprint-branch sync")).toBe(0)
      syncExited = true
    } finally {
      if (!syncExited) {
        syncProcess.kill("SIGTERM")
        await syncProcess.exited
      }
    }
  })

  test("sync --replace stops a same-directory sync before starting", async () => {
    const { reviewWorktree } = await createSprintSyncFixture()
    const syncProcess = spawnCli(reviewWorktree, ["sync", "--sprint", "example", "--json"])
    let syncExited = false
    let replacementExited = false

    try {
      await waitForReviewSyncCheckout(reviewWorktree)
      const replacement = spawnCli(reviewWorktree, [
        "sync",
        "--sprint",
        "example",
        "--replace",
        "--json",
      ])

      try {
        await waitForCliExit(syncProcess, "original sprint-branch sync")
        syncExited = true

        await waitForActiveReviewSyncSession(reviewWorktree)
        const stopResult = await runCli(reviewWorktree, ["stop-sync", "--json"])
        const stopped = JSON.parse(stopResult.stdout) as SprintSyncStopReport
        expect(stopped.stopped).toBe(1)

        expect(await waitForCliExit(replacement, "replacement sprint-branch sync")).toBe(0)
        replacementExited = true
        const [stdout, stderr] = await Promise.all([
          new Response(replacement.stdout).text(),
          new Response(replacement.stderr).text(),
        ])
        const sync = JSON.parse(stdout) as SyncOutput

        expect(stderr).toBe("")
        expect(sync.ok).toBe(true)
        expect(sync.reviewSync?.command).toBe("watch")
        expect(sync.reviewSync?.status).toBe("paused")
      } finally {
        if (!replacementExited) {
          replacement.kill("SIGTERM")
          await replacement.exited
        }
      }
    } finally {
      if (!syncExited) {
        syncProcess.kill("SIGTERM")
        await syncProcess.exited
      }
    }
  })
})

function createDeferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function createSprintSyncFixture() {
  const repo = await createSprintRepo("example", {
    review: "010-task-name",
    next: null,
    approved: [],
  })
  const reviewWorktree = await createLinkedWorktree(repo, "main")
  await git(repo, ["checkout", "sprint/example/review"])
  await fs.writeFile(path.join(repo, "feature.txt"), "agent review work\n")
  return { repo, reviewWorktree }
}

async function waitForReviewSyncCheckout(reviewWorktree: string) {
  await waitUntil(
    async () => (await currentBranch(reviewWorktree)) === "review-sync/sprint/example/review",
    "sync did not check out the review-sync branch",
  )
}

async function waitForActiveReviewSyncSession(reviewWorktree: string) {
  await waitUntil(
    async () =>
      (await findRunningSprintSyncs({ cwd: reviewWorktree })).syncs.length > 0 &&
      (await statusReviewSession({ cwd: reviewWorktree })).status === "ok",
    "replacement sync did not start watching before the timeout",
  )
}

async function waitUntil(check: () => Promise<boolean>, message: string) {
  const deadline = Date.now() + 5000
  while (Date.now() < deadline) {
    if (await check()) {
      return
    }
    await Bun.sleep(50)
  }
  throw new Error(message)
}

async function waitForCliExit(subprocess: ReturnType<typeof spawnCli>, command: string) {
  let timeout: ReturnType<typeof setTimeout> | null = null
  try {
    return await Promise.race([
      subprocess.exited,
      new Promise<number>((_, reject) => {
        timeout = setTimeout(() => {
          subprocess.kill("SIGTERM")
          reject(new Error(`${command} did not exit within 5000ms`))
        }, 5000)
      }),
    ])
  } finally {
    if (timeout) {
      clearTimeout(timeout)
    }
  }
}

async function exitsWithin(subprocess: ReturnType<typeof spawnCli>, timeoutMs: number) {
  return await Promise.race([
    subprocess.exited.then(() => true),
    Bun.sleep(timeoutMs).then(() => false),
  ])
}
