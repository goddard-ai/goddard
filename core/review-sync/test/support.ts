import { spawn } from "node:child_process"
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { expect } from "bun:test"

import { UserError } from "../src/errors.ts"
import { startReviewSync, watchReviewSession, type ReviewSyncResult } from "../src/index.ts"

export type ReviewSyncFixture = {
  rootDir: string
  agentDir: string
  reviewDir: string
}

type StoredSessionState = {
  sessionId: string
  agentBranch: string
  reviewBranch: string
}

const fixtureCleanup: string[] = []
export const cliPath = fileURLToPath(new URL("../src/cli.ts", import.meta.url))

export async function cleanupReviewSyncFixtures() {
  while (fixtureCleanup.length > 0) {
    await rm(fixtureCleanup.pop()!, { recursive: true, force: true })
  }
}

export async function runWatchUntilNextSync(
  cwd: string,
  mutate: () => Promise<void>,
  agentBranch?: string,
) {
  const controller = new AbortController()
  const timeoutReason = "watch test timeout"
  const timeout = setTimeout(() => controller.abort(timeoutReason), 4000)
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
    // macOS can deliver a one-shot write before fs.watch has fully armed.
    await sleep(100)
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

export async function createStartedFixture(files: Record<string, string>) {
  const fixture = await createFixture(files)
  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })
  expect(result.status).toBe("ok")
  return fixture
}

export async function createFixture(files: Record<string, string>) {
  const rootDir = await mkdtemp(join(tmpdir(), "review-sync-test-"))
  const agentDir = join(rootDir, "agent")
  const reviewDir = join(rootDir, "review")
  fixtureCleanup.push(rootDir)

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

export async function writeText(path: string, content: string) {
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, content, "utf-8")
}

export async function currentBranch(cwd: string) {
  return (await runGit(cwd, ["symbolic-ref", "--short", "HEAD"])).stdout.trim()
}

export async function gitDir(cwd: string) {
  return (await runGit(cwd, ["rev-parse", "--path-format=absolute", "--git-dir"])).stdout.trim()
}

export async function readSessionStates(cwd: string) {
  const commonDir = (
    await runGit(cwd, ["rev-parse", "--path-format=absolute", "--git-common-dir"])
  ).stdout.trim()
  const sessionsRoot = join(commonDir, "review-sync", "sessions")
  let entries: string[]
  try {
    entries = await readdir(sessionsRoot)
  } catch {
    return []
  }
  const states = await Promise.all(
    entries.map(async (entry) => {
      return JSON.parse(
        await readFile(join(sessionsRoot, entry, "state.json"), "utf-8"),
      ) as StoredSessionState
    }),
  )
  return [...states].sort((left, right) => left.sessionId.localeCompare(right.sessionId))
}

export async function refExists(cwd: string, refName: string) {
  const result = await runProcess(cwd, "git", ["rev-parse", "--verify", "-q", refName])
  return result.status === 0
}

export function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}

export async function waitForFileContent(path: string, expected: string) {
  const startedAt = Date.now()
  while (Date.now() - startedAt < 5000) {
    if ((await readFile(path, "utf-8")) === expected) {
      return
    }
    await sleep(50)
  }
  expect(await readFile(path, "utf-8")).toBe(expected)
}

export function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

export async function captureReviewSyncError(operation: () => Promise<unknown>) {
  let captured: unknown
  try {
    await operation()
  } catch (error) {
    captured = error
  }
  expect(captured).toBeInstanceOf(UserError)
  return captured as UserError
}

export async function runGit(cwd: string, args: string[]) {
  const result = await runProcess(cwd, "git", args)
  if (result.status !== 0) {
    throw new Error(
      `git ${args.join(" ")} failed in ${cwd}: ${result.stderr.trim() || result.stdout.trim()}`,
    )
  }
  return result
}

export async function runProcess(cwd: string, command: string, args: string[]) {
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

export async function runProcessUntilOutput(
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
