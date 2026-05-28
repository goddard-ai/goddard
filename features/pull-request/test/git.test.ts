import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "../src/daemon/git.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  await Promise.all(cleanup.splice(0).map((dispose) => dispose()))
})

test("pull request git helpers infer repository and branch defaults", async () => {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-pr-git-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.name", "Goddard"])
  runGit(repoDir, ["config", "user.email", "goddard@example.com"])
  await writeFile(join(repoDir, "README.md"), "# test\n", "utf-8")
  runGit(repoDir, ["add", "README.md"])
  runGit(repoDir, ["commit", "-m", "init"])
  runGit(repoDir, ["checkout", "-b", "feature/ipc"])
  runGit(repoDir, ["remote", "add", "origin", "git@github.com:acme/widgets.git"])
  await mkdir(join(repoDir, ".git", "refs", "remotes", "origin"), {
    recursive: true,
  })
  await writeFile(
    join(repoDir, ".git", "refs", "remotes", "origin", "HEAD"),
    "ref: refs/remotes/origin/main\n",
  )

  const submit = await resolveSubmitRequestFromGit({
    cwd: repoDir,
    title: "Implement IPC routing",
    body: "Done.",
  })
  expect(submit).toEqual({
    owner: "acme",
    repo: "widgets",
    title: "Implement IPC routing",
    body: "Done.",
    head: "feature/ipc",
    base: "main",
  })

  runGit(repoDir, ["checkout", "-B", "pr-12"])
  const reply = await resolveReplyRequestFromGit({
    cwd: repoDir,
    message: "Updated per review",
  })
  expect(reply).toEqual({
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    body: "Updated per review",
  })
})

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  if (result.status !== 0) {
    throw new Error(result.stderr || `git ${args.join(" ")} failed`)
  }
}
