import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  git,
  readState,
  runCli,
  writeState,
} from "./support"

/** Machine-readable checkout payload used by subprocess CLI tests. */
type CheckoutOutput = {
  ok: boolean
  dryRun: boolean
  executed: boolean
  detached: boolean
  sprint: string | null
  reviewBranch: string | null
  commit: string | null
  gitOperations: string[]
  diagnostics: Array<{ code: string }>
  candidates: Array<{ sprint: string; reviewBranch: string }>
}

describe("sprint-branch checkout", () => {
  afterEach(cleanupTestRepos)

  test("checks out the requested sprint review branch as detached HEAD", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")
    await git(repo, ["checkout", "main"])
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["checkout", "example", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(0)
    expect(checkout.ok).toBe(true)
    expect(checkout.executed).toBe(true)
    expect(checkout.detached).toBe(true)
    expect(checkout.sprint).toBe("example")
    expect(checkout.reviewBranch).toBe("sprint/example/review")
    expect((await git(repo, ["rev-parse", "--abbrev-ref", "HEAD"])).trim()).toBe("HEAD")
    expect(await branchHead(repo, "HEAD")).toBe(await branchHead(repo, "sprint/example/review"))
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("refuses non-interactive checkout without a strong sprint context", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })

    const result = await runCli(repo, ["checkout", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(1)
    expect(checkout.ok).toBe(false)
    expect(diagnosticCodes(checkout)).toContain("sprint_selection_required")
    expect(checkout.candidates.map((candidate) => candidate.sprint)).toEqual(["example"])
    expect(await currentBranch(repo)).toBe("main")
  })

  // In the primary human clone, checkout must not guess which sprint to review.
  // Non-interactive callers get a candidate list and can retry by name.
  test("reports all candidates for non-interactive checkout without a sprint name", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await addSprint(repo, "other")

    const result = await runCli(repo, ["checkout", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(1)
    expect(checkout.ok).toBe(false)
    expect(diagnosticCodes(checkout)).toContain("sprint_selection_required")
    expect(checkout.candidates.map((candidate) => candidate.sprint)).toEqual(["example", "other"])
    expect(await currentBranch(repo)).toBe("main")
  })

  // The command is for clean review snapshots, not for carrying local edits
  // across branches. Refusing dirty work avoids mixing unrelated human work into review.
  test("refuses checkout with a dirty working tree", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ndirty\n")

    const result = await runCli(repo, ["checkout", "example", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(checkout)).toContain("dirty_worktree")
    expect(await currentBranch(repo)).toBe("main")
  })

  // Dry-run gives humans and scripts the exact detached checkout target without
  // moving the worktree away from their current branch.
  test("dry-run does not detach HEAD", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })

    const result = await runCli(repo, ["checkout", "example", "--dry-run", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(0)
    expect(checkout.dryRun).toBe(true)
    expect(checkout.executed).toBe(false)
    expect(checkout.gitOperations).toEqual(["git checkout --detach sprint/example/review"])
    expect(await currentBranch(repo)).toBe("main")
  })
})

/** Adds a second valid sprint state and review branch to make checkout inference ambiguous. */
async function addSprint(repo: string, sprint: string) {
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  await fs.writeFile(path.join(repo, "sprints", sprint, "010-task-name.md"), "# Task 010\n")
  await writeState(repo, sprint, sprintState(repo, sprint))
  await commitAll(repo, `add ${sprint} sprint`)
  await git(repo, ["branch", `sprint/${sprint}/approved`, "main"])
  await git(repo, ["branch", `sprint/${sprint}/review`, `sprint/${sprint}/approved`])
}

/** Builds minimal canonical state for a test-only sprint. */
function sprintState(repo: string, sprint: string) {
  return {
    sprint,
    baseBranch: "main",
    sprintWorktreeRoot: repo,
    tasks: {
      review: "010-task-name",
      next: null,
      approved: [],
    },
    activeStashes: [],
    conflict: null,
  }
}
