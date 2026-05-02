import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { runSprintSync, type SprintSyncReport } from "../src"
import {
  cleanupTestRepos,
  createLinkedWorktree,
  createSprintRepo,
  currentBranch,
  git,
  runCli,
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
      expect(sync.reviewSync?.status).toBe("ok")
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
