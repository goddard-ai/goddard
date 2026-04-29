import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  pauseReviewSession,
  resumeReviewSession,
  startReviewSync,
  syncReviewSession,
} from "../src/commands.ts"

type ReviewSyncFixture = {
  rootDir: string
  agentDir: string
  reviewDir: string
}

const cleanup: string[] = []

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
    cwd: fixture.agentDir,
    reviewWorktree: fixture.reviewDir,
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
    cwd: fixture.agentDir,
    reviewWorktree: fixture.reviewDir,
  })

  expect(result.status).toBe("error")
  expect(result.exitCode).toBe(1)
  expect(result.message).toContain("already ends with --review")
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

async function createStartedFixture(files: Record<string, string>) {
  const fixture = await createFixture(files)
  const result = await startReviewSync({
    cwd: fixture.agentDir,
    reviewWorktree: fixture.reviewDir,
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

async function runGit(cwd: string, args: string[]) {
  return await new Promise<{
    status: number
    stdout: string
    stderr: string
  }>((resolvePromise, rejectPromise) => {
    const child = spawn("git", args, {
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
      const result = {
        status: status ?? 1,
        stdout,
        stderr,
      }
      if (result.status !== 0) {
        rejectPromise(
          new Error(
            `git ${args.join(" ")} failed in ${cwd}: ${
              result.stderr.trim() || result.stdout.trim()
            }`,
          ),
        )
        return
      }
      resolvePromise(result)
    })
  })
}
