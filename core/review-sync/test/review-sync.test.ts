import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { afterEach, expect, test } from "bun:test"

import {
  pauseReviewSession,
  resumeReviewSession,
  startReviewSync,
  syncReviewSession,
  watchReviewSession,
  type ReviewSyncResult,
} from "../src/index.ts"

type ReviewSyncFixture = {
  rootDir: string
  agentDir: string
  reviewDir: string
}

const cleanup: string[] = []
const cliPath = fileURLToPath(new URL("../src/cli.ts", import.meta.url))

afterEach(async () => {
  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("start derives and checks out the review branch", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })

  expect(result.status).toBe("ok")
  expect(result.reviewBranch).toBe("codex/review-sync-test--review")
  expect(await currentBranch(fixture.reviewDir)).toBe("codex/review-sync-test--review")
})

test("start refuses agent branches that already look like review branches", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/already--review"])

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/already--review",
  })

  expect(result.status).toBe("error")
  expect(result.exitCode).toBe(1)
  expect(result.message).toContain("already ends with --review")
})

test("start refuses branches not checked out in another worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/other-agent"])

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })

  expect(result.status).toBe("error")
  expect(result.exitCode).toBe(1)
  expect(result.message).toContain("is not checked out in another worktree")
})

test("cli start accepts an agent branch from the review worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcess(fixture.reviewDir, "bun", [
    cliPath,
    "start",
    "codex/review-sync-test",
  ])

  expect(result.status).toBe(0)
  expect(result.stdout).toContain("Started review sync")
  expect(await currentBranch(fixture.reviewDir)).toBe("codex/review-sync-test--review")
})

test("cli start requires an agent branch when non-interactive", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcess(fixture.reviewDir, "bun", [cliPath, "start"])

  expect(result.status).toBe(1)
  expect(result.stderr).toContain("start requires an agent branch")
})

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
  expect(await currentBranch(fixture.reviewDir)).toBe("codex/review-sync-test--review")
})

test("sync mirrors agent uncommitted changes through the review branch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  const result = await syncReviewSession({
    cwd: fixture.agentDir,
  })

  expect(result.status).toBe("ok")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
})

test("sync applies clean human edits back to the agent worktree", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(result.acceptedPatchPath).toBeTruthy()
  expect(existsSync(result.acceptedPatchPath!)).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("sync waits for the expected agent checkout before mutating worktrees", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const controller = new AbortController()
  const timeout = setTimeout(() => {
    controller.abort("test timeout")
  }, 5000)
  let settled = false
  const sync = syncReviewSession({
    cwd: fixture.reviewDir,
    checkoutWaitIntervalMs: 10,
    signal: controller.signal,
  }).finally(() => {
    clearTimeout(timeout)
    settled = true
  })

  await sleep(50)
  expect(settled).toBe(false)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("base\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")

  await runGit(fixture.agentDir, ["checkout", "codex/review-sync-test"])
  const result = await sync

  expect(result.status).toBe("ok")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("sync preserves rejected human patches and refreshes review from the agent", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("rejected-human-patch")
  expect(result.rejectedPatchPath).toBeTruthy()
  expect(await readFile(result.rejectedPatchPath!, "utf-8")).toContain("human edit")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
})

test("sync includes untracked non-ignored files and excludes ignored files", async () => {
  const fixture = await createStartedFixture({
    ".gitignore": "ignored.txt\n",
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "new-file.txt"), "include me\n")
  await writeText(join(fixture.reviewDir, "ignored.txt"), "do not include\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(await readFile(join(fixture.agentDir, "new-file.txt"), "utf-8")).toBe("include me\n")
  expect(existsSync(join(fixture.agentDir, "ignored.txt"))).toBe(false)
})

test("pause blocks sync mutations until resume", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  const pause = await pauseReviewSession({
    cwd: fixture.agentDir,
  })
  expect(pause.status).toBe("paused")

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const pausedSync = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(pausedSync.status).toBe("paused")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("base\n")

  const resume = await resumeReviewSession({
    cwd: fixture.agentDir,
  })
  expect(resume.status).toBe("ok")

  const synced = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(synced.status).toBe("ok")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
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

  expect(stopped.status).toBe("ok")
  expect(results.some((result) => result.command === "start" && result.status === "ok")).toBe(true)
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
  expect(await currentBranch(fixture.reviewDir)).toBe("codex/review-sync-test--review")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("watch syncs when the review worktree changes", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const { results, stopped } = await runWatchUntilNextSync(fixture.reviewDir, async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  })

  expect(stopped.status).toBe("ok")
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

  expect(stopped.status).toBe("ok")
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

  expect(stopped.status).toBe("ok")
  expect(results.some((result) => result.command === "sync" && result.status === "ok")).toBe(true)
})

async function runWatchUntilNextSync(
  cwd: string,
  mutate: () => Promise<void>,
  agentBranch?: string,
) {
  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 5000)
  const started = createDeferred<void>()
  let startedResolved = false
  const results: ReviewSyncResult[] = []
  const watch = watchReviewSession({
    cwd,
    agentBranch,
    signal: controller.signal,
    onResult: (result) => {
      results.push(result)
      if (result.command === "watch") {
        startedResolved = true
        started.resolve()
      }
      if (result.command === "sync" && result.status === "ok") {
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
    await mutate()
    const stopped = await watch
    if (controller.signal.reason === timeoutReason) {
      throw new Error("watch timed out before observing a sync result")
    }
    return { results, stopped }
  } finally {
    clearTimeout(timeout)
  }
}

async function createStartedFixture(files: Record<string, string>) {
  const fixture = await createFixture(files)
  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })
  expect(result.status).toBe("ok")
  return fixture
}

async function createFixture(files: Record<string, string>) {
  const rootDir = await mkdtemp(join(tmpdir(), "review-sync-test-"))
  const agentDir = join(rootDir, "agent")
  const reviewDir = join(rootDir, "review")
  cleanup.push(rootDir)

  await mkdir(agentDir, { recursive: true })
  await runGit(agentDir, ["init", "-b", "main"])
  await runGit(agentDir, ["config", "user.email", "bot@example.com"])
  await runGit(agentDir, ["config", "user.name", "Bot"])

  for (const [path, content] of Object.entries(files)) {
    await writeText(join(agentDir, path), content)
  }

  await runGit(agentDir, ["add", "."])
  await runGit(agentDir, ["commit", "-m", "init"])
  await runGit(agentDir, ["checkout", "-B", "codex/review-sync-test"])
  await runGit(agentDir, ["worktree", "add", "--detach", reviewDir, "HEAD"])

  return {
    rootDir,
    agentDir,
    reviewDir,
  } satisfies ReviewSyncFixture
}

async function writeText(path: string, content: string) {
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, content, "utf-8")
}

async function currentBranch(cwd: string) {
  return (await runGit(cwd, ["symbolic-ref", "--short", "HEAD"])).stdout.trim()
}

function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function runGit(cwd: string, args: string[]) {
  const result = await runProcess(cwd, "git", args)
  if (result.status !== 0) {
    throw new Error(
      `git ${args.join(" ")} failed in ${cwd}: ${result.stderr.trim() || result.stdout.trim()}`,
    )
  }
  return result
}

async function runProcess(cwd: string, command: string, args: string[]) {
  return await new Promise<{
    status: number
    stdout: string
    stderr: string
  }>((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    })

    let stdout = ""
    let stderr = ""
    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })
  })
}

async function runProcessUntilOutput(
  cwd: string,
  command: string,
  args: string[],
  expectedOutput: string,
) {
  return await new Promise<{
    status: number
    stdout: string
    stderr: string
  }>((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    })

    let matched = false
    let stdout = ""
    let stderr = ""
    const timeout = setTimeout(() => {
      child.kill("SIGKILL")
      rejectPromise(new Error(`Timed out waiting for ${expectedOutput}`))
    }, 5000)
    const stopWhenMatched = () => {
      if (!matched && `${stdout}\n${stderr}`.includes(expectedOutput)) {
        matched = true
        child.kill("SIGTERM")
      }
    }

    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
      stopWhenMatched()
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
      stopWhenMatched()
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      clearTimeout(timeout)
      if (!matched) {
        rejectPromise(new Error(`Process exited before printing ${expectedOutput}: ${stderr}`))
        return
      }
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })
  })
}
