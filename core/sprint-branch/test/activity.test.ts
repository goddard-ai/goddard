import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  git,
  readState,
  runCli,
  writeState,
} from "./support"

/** Machine-readable status payload fields used by activity selection tests. */
type StatusOutput = {
  ok: boolean
  sprint: string
  inferredFrom: string
  state: { lastActedAt: string | null }
}

/** Machine-readable list payload fields used by activity selection tests. */
type ListOutput = {
  ok: boolean
  lastOnly: boolean
  sprints: Array<{ sprint: string; lastActedAt: string | null }>
  diagnostics: Array<{ code: string }>
}

/** Machine-readable checkout payload fields used by activity selection tests. */
type CheckoutOutput = {
  ok: boolean
  sprint: string | null
}

describe("sprint-branch activity selection", () => {
  afterEach(cleanupTestRepos)

  test("records sprint activity and resolves -l from the latest acted sprint", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await addSprint(repo, "other", null)

    const inspected = await runCli(repo, ["status", "--sprint", "example", "--json"])
    const inspectedStatus = JSON.parse(inspected.stdout) as StatusOutput
    const inspectedState = await readState(repo, "example")

    expect(inspected.exitCode).toBe(0)
    expect(inspectedStatus.ok).toBe(true)
    expect(typeof inspectedStatus.state.lastActedAt).toBe("string")
    expect(inspectedStatus.state.lastActedAt).toBe(inspectedState.lastActedAt)

    const latest = await runCli(repo, ["status", "-l", "--json"])
    const latestStatus = JSON.parse(latest.stdout) as StatusOutput

    expect(latest.exitCode).toBe(0)
    expect(latestStatus.sprint).toBe("example")
    expect(latestStatus.inferredFrom).toContain("last sprint")
  })

  test("orders list output by last activity and supports list -l", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await writeState(
      repo,
      "example",
      sprintState(repo, "example", "active", "2026-01-01T00:00:00.000Z"),
    )
    await addSprint(repo, "other", "2026-02-01T00:00:00.000Z")

    const listed = await runCli(repo, ["list", "--json"])
    const list = JSON.parse(listed.stdout) as ListOutput
    const lastListed = await runCli(repo, ["list", "-l", "--json"])
    const lastList = JSON.parse(lastListed.stdout) as ListOutput

    expect(listed.exitCode).toBe(0)
    expect(list.sprints.map((sprint) => sprint.sprint)).toEqual(["other", "example"])
    expect(lastListed.exitCode).toBe(0)
    expect(lastList.lastOnly).toBe(true)
    expect(lastList.sprints.map((sprint) => sprint.sprint)).toEqual(["other"])
  })

  test("checkout -l uses the latest acted sprint", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await writeState(
      repo,
      "example",
      sprintState(repo, "example", "active", "2026-01-01T00:00:00.000Z"),
    )
    await addSprint(repo, "other", "2026-02-01T00:00:00.000Z")

    const result = await runCli(repo, ["checkout", "-l", "--dry-run", "--json"])
    const checkout = JSON.parse(result.stdout) as CheckoutOutput

    expect(result.exitCode).toBe(0)
    expect(checkout.ok).toBe(true)
    expect(checkout.sprint).toBe("other")
  })
})

/** Adds a valid active sprint state and review branches for selection tests. */
async function addSprint(repo: string, sprint: string, lastActedAt: string | null) {
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  await fs.writeFile(path.join(repo, "sprints", sprint, "010-task-name.md"), "# Task 010\n")
  await writeState(repo, sprint, sprintState(repo, sprint, "active", lastActedAt))
  await commitAll(repo, `add ${sprint} sprint`)
  await git(repo, ["branch", `sprint/${sprint}/approved`, "main"])
  await git(repo, ["branch", `sprint/${sprint}/review`, `sprint/${sprint}/approved`])
}

/** Builds minimal canonical state for a test-only sprint. */
function sprintState(
  repo: string,
  sprint: string,
  visibility: "active" | "parked",
  lastActedAt: string | null,
) {
  return {
    sprint,
    baseBranch: "main",
    sprintWorktreeRoot: repo,
    visibility,
    lastActedAt,
    tasks: {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    },
    activeStashes: [],
    conflict: null,
  }
}
