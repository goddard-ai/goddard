import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchExists,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  diagnosticCodes,
  git,
  readState,
  runCli,
  writeState,
  type MutationOutput,
} from "./support"

type ListOutput = {
  ok: boolean
  sprints: Array<{ sprint: string; visibility: string; reviewBranch: string }>
}

type CheckoutOutput = {
  ok: boolean
  sprint: string | null
  candidates: Array<{ sprint: string }>
  diagnostics: Array<{ code: string }>
}

describe("sprint-branch park", () => {
  afterEach(cleanupTestRepos)

  test("parks and unparks sprint state without deleting branches", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })

    const parkResult = await runCli(repo, ["park", "--sprint", "example", "--json"])
    const park = JSON.parse(parkResult.stdout) as MutationOutput

    expect(parkResult.exitCode).toBe(0)
    expect(park.executed).toBe(true)
    expect(park.gitOperations).toEqual([])
    expect((await readState(repo, "example")).visibility).toBe("parked")
    expect(await branchExists(repo, "sprint/example/review")).toBe(true)
    expect(await branchExists(repo, "sprint/example/approved")).toBe(true)

    const unparkResult = await runCli(repo, ["unpark", "--sprint", "example", "--json"])
    const unpark = JSON.parse(unparkResult.stdout) as MutationOutput

    expect(unparkResult.exitCode).toBe(0)
    expect(unpark.executed).toBe(true)
    expect(unpark.gitOperations).toEqual([])
    expect((await readState(repo, "example")).visibility).toBe("active")
  })

  test("lists active sprints by default and parked sprints with all", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await addSprint(repo, "other", "parked")

    const activeResult = await runCli(repo, ["list", "--json"])
    const active = JSON.parse(activeResult.stdout) as ListOutput
    const allResult = await runCli(repo, ["list", "--all", "--json"])
    const all = JSON.parse(allResult.stdout) as ListOutput

    expect(activeResult.exitCode).toBe(0)
    expect(active.ok).toBe(true)
    expect(active.sprints.map((sprint) => sprint.sprint)).toEqual(["example"])
    expect(allResult.exitCode).toBe(0)
    expect(all.sprints.map((sprint) => [sprint.sprint, sprint.visibility])).toEqual([
      ["example", "active"],
      ["other", "parked"],
    ])
  })

  test("omits parked sprints from default candidates but still allows strong context", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await addSprint(repo, "other", "parked")

    const checkoutResult = await runCli(repo, ["checkout", "--json"])
    const checkout = JSON.parse(checkoutResult.stdout) as CheckoutOutput

    expect(checkoutResult.exitCode).toBe(1)
    expect(checkout.ok).toBe(false)
    expect(diagnosticCodes(checkout)).toContain("sprint_selection_required")
    expect(checkout.candidates.map((candidate) => candidate.sprint)).toEqual(["example"])

    const explicitResult = await runCli(repo, ["checkout", "other", "--dry-run", "--json"])
    const explicit = JSON.parse(explicitResult.stdout) as CheckoutOutput

    expect(explicitResult.exitCode).toBe(0)
    expect(explicit.ok).toBe(true)
    expect(explicit.sprint).toBe("other")

    const pathResult = await runCli(path.join(repo, "sprints", "other"), ["status", "--json"])
    const status = JSON.parse(pathResult.stdout) as {
      ok: boolean
      sprint: string
      visibility: string
    }

    expect(pathResult.exitCode).toBe(0)
    expect(status.ok).toBe(true)
    expect(status.sprint).toBe("other")
    expect(status.visibility).toBe("parked")
  })
})

/** Adds a valid sprint state and review branches with the requested visibility. */
async function addSprint(repo: string, sprint: string, visibility: "active" | "parked") {
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  await fs.writeFile(path.join(repo, "sprints", sprint, "010-task-name.md"), "# Task 010\n")
  await writeState(repo, sprint, sprintState(repo, sprint, visibility))
  await commitAll(repo, `add ${sprint} sprint`)
  await git(repo, ["branch", `sprint/${sprint}/approved`, "main"])
  await git(repo, ["branch", `sprint/${sprint}/review`, `sprint/${sprint}/approved`])
}

/** Builds minimal canonical state for a test-only sprint. */
function sprintState(repo: string, sprint: string, visibility: "active" | "parked") {
  return {
    sprint,
    baseBranch: "main",
    sprintWorktreeRoot: repo,
    visibility,
    tasks: {
      review: "010-task-name",
      next: null,
      approved: [],
    },
    activeStashes: [],
    conflict: null,
  }
}
