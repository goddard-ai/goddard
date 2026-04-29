import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"

import { startReviewSync, statusReviewSession, syncReviewSession } from "../src/index.ts"

type SmokeFixture = {
  rootDir: string
  agentDir: string
  reviewDir: string
}

let rootDir: string | null = null

try {
  const fixture = await createFixture()
  rootDir = fixture.rootDir

  const start = await startReviewSync({
    cwd: fixture.agentDir,
    reviewWorktree: fixture.reviewDir,
  })
  assertEqual(start.status, "ok", "start status")
  assertEqual(start.reviewBranch, "codex/review-sync-smoke--review", "review branch")
  assertEqual(
    await currentBranch(fixture.reviewDir),
    "codex/review-sync-smoke--review",
    "review worktree branch",
  )

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const sync = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  assertEqual(sync.status, "ok", "sync status")
  assertTruthy(sync.acceptedPatchPath, "accepted patch path")
  assertTruthy(existsSync(sync.acceptedPatchPath!), "accepted patch file exists")
  assertEqual(
    await readFile(join(fixture.agentDir, "shared.txt"), "utf-8"),
    "human edit\n",
    "agent file content",
  )
  assertEqual(
    await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8"),
    "human edit\n",
    "review file content",
  )

  const status = await statusReviewSession({
    cwd: fixture.agentDir,
    json: true,
  })
  assertEqual(status.status, "ok", "status command")
  const statusPayload = JSON.parse(status.message) as {
    patchCounts?: {
      accepted?: number
      rejected?: number
    }
  }
  assertEqual(statusPayload.patchCounts?.accepted, 1, "accepted patch count")
  assertEqual(statusPayload.patchCounts?.rejected, 0, "rejected patch count")

  await rm(fixture.rootDir, { recursive: true, force: true })
  rootDir = null
  console.log("review-sync smoke happy path passed")
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  if (rootDir) {
    console.error(`smoke fixture preserved at ${rootDir}`)
  }
  process.exitCode = 1
}

async function createFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "review-sync-smoke-"))
  const agentDir = join(rootDir, "agent")
  const reviewDir = join(rootDir, "review")

  await mkdir(agentDir, { recursive: true })
  await runGit(agentDir, ["init", "-b", "main"])
  await runGit(agentDir, ["config", "user.email", "smoke@example.com"])
  await runGit(agentDir, ["config", "user.name", "Smoke Test"])
  await writeText(join(agentDir, "shared.txt"), "base\n")
  await runGit(agentDir, ["add", "."])
  await runGit(agentDir, ["commit", "-m", "init"])
  await runGit(agentDir, ["checkout", "-B", "codex/review-sync-smoke"])
  await runGit(agentDir, ["worktree", "add", "--detach", reviewDir, "HEAD"])

  return {
    rootDir,
    agentDir,
    reviewDir,
  } satisfies SmokeFixture
}

async function writeText(path: string, content: string) {
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, content, "utf-8")
}

async function currentBranch(cwd: string) {
  return (await runGit(cwd, ["symbolic-ref", "--short", "HEAD"])).stdout.trim()
}

function assertEqual(actual: unknown, expected: unknown, label: string) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`)
  }
}

function assertTruthy(value: unknown, label: string) {
  if (!value) {
    throw new Error(`${label}: expected truthy value`)
  }
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
