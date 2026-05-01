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

describe("sprint-branch doctor", () => {
  afterEach(cleanupTestRepos)

  // A next task in JSON means there should be resumable work on the next branch.
  // Doctor should catch the missing ref before resume has to guess what happened.
  test("reports missing next branch when next has a recorded task", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: "020-task-name",
      approved: [],
    })

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as {
      ok: boolean
      diagnostics: Array<{ code: string }>
    }

    expect(result.exitCode).toBe(1)
    expect(doctor.ok).toBe(false)
    expect(diagnosticCodes(doctor)).toContain("next_branch_missing")
  })

  // Review commits without a recorded task break the one-task-per-review invariant.
  // Doctor surfaces that hidden work so it is not approved or finalized anonymously.
  test("reports unrecorded review branch work when no review task is assigned", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "unrecorded.txt"), "review work\n")
    await commitAll(repo, "add unrecorded review work")

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(doctor)).toContain("review_branch_has_unrecorded_work")
  })

  // Task role bugs are subtle because the branches may still exist and be well-formed.
  // Doctor checks the logical queue so one task cannot occupy multiple workflow states.
  test("reports duplicate task assignments and task order drift", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "020-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput
    const codes = diagnosticCodes(doctor)

    expect(result.exitCode).toBe(1)
    expect(codes).toContain("task_assigned_multiple_roles")
    expect(codes).toContain("review_task_out_of_order")
  })

  // Sprint stashes are only safe to reapply when JSON records their branch and task.
  // Unrecorded matching stashes are recovery clues, not inputs resume should trust blindly.
  test("reports unrecorded sprint-branch stashes", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "scratch.txt"), "interrupted\n")
    await git(repo, [
      "stash",
      "push",
      "--include-untracked",
      "-m",
      "sprint-branch:example:020-task-name:feedback",
    ])

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(diagnosticCodes(doctor)).toContain("unrecorded_sprint_stash")
  })

  // Git can be left mid-rebase even if the sprint state was never updated.
  // Doctor must detect the repository-level operation so agents do not start a new transition.
  test("reports git operations that are not reflected in sprint state", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await gitAllowFailure(repo, ["checkout", "sprint/example/next"])
    await gitAllowFailure(repo, ["rebase", "sprint/example/review"])

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(doctor)).toContain("git_operation_without_conflict_state")
  })

  // Transactional commands keep canonical sprint state untouched while Git owns a
  // rebase. Doctor should recognize the Git-private marker as known recovery
  // state instead of claiming the sprint JSON forgot about the operation.
  test("recognizes transient conflict metadata during an active rebase", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")

    expect((await runCli(repo, ["approve", "--sprint", "example", "--json"])).exitCode).toBe(1)

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput
    const codes = diagnosticCodes(doctor)

    expect(result.exitCode).toBe(1)
    expect(codes).toContain("git_operation_in_progress")
    expect(codes).not.toContain("git_operation_without_conflict_state")
    expect(doctor.nextSafeCommand).toBe("sprint-branch approve --dry-run")
  })

  // After the human or agent finishes Git's final rebase, review is expected to
  // differ from approved until finalize is rerun. Doctor should point at the
  // pending retry instead of reporting anonymous review work.
  test("recognizes finalize retry state after final rebase is resolved", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "main\n")
    await commitAll(repo, "add main conflict")
    expect((await runCli(repo, ["finalize", "--sprint", "example", "--json"])).exitCode).toBe(1)
    await fs.writeFile(path.join(repo, "conflict.txt"), "resolved\n")
    await git(repo, ["add", "conflict.txt"])
    await git(repo, ["-c", "core.editor=true", "rebase", "--continue"])

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput
    const codes = diagnosticCodes(doctor)

    expect(result.exitCode).toBe(1)
    expect(codes).toContain("transition_retry_pending")
    expect(codes).not.toContain("review_not_based_on_approved")
    expect(codes).not.toContain("review_branch_has_unrecorded_work")
    expect(doctor.nextSafeCommand).toBe("sprint-branch finalize --dry-run")
  })

  // A stash-apply conflict does not leave Git with a rebase or merge operation
  // to continue. The recorded resume conflict is still valid while the worktree
  // carries the applied stash resolution and the stash remains recorded.
  test("recognizes resume stash-apply conflicts without a git operation", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "work.txt"), "base\n")
    await commitAll(repo, "add shared work file")
    await git(repo, ["checkout", "sprint/example/next"])
    await git(repo, ["rebase", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "work.txt"), "stashed\n")
    expect((await runCli(repo, ["feedback", "--sprint", "example"])).exitCode).toBe(0)
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "work.txt"), "review\n")
    await commitAll(repo, "change work during feedback")
    expect((await runCli(repo, ["resume", "--sprint", "example", "--json"])).exitCode).toBe(1)

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput
    const codes = diagnosticCodes(doctor)

    expect(result.exitCode).toBe(1)
    expect(codes).toContain("transition_retry_pending")
    expect(codes).not.toContain("conflict_state_without_git_operation")
    expect(doctor.nextSafeCommand).toBe("sprint-branch resume --dry-run")
  })

  // Conflict metadata should match a real paused Git operation.
  // Stale conflict state would keep all mutating commands blocked after recovery is complete.
  test("reports stale conflict state when Git has no operation in progress", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    const state = await readState(repo, "example")
    await writeState(repo, "example", {
      ...state,
      conflict: {
        command: "resume",
        branch: "sprint/example/review",
        message: "stale conflict",
      },
    })

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(doctor)).toContain("conflict_state_without_git_operation")
  })

  // Finalize depends on the recorded base branch still existing.
  // Doctor checks it early so recovery can choose an explicit base before rebasing review.
  test("reports a missing recorded base branch", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await git(repo, ["branch", "-D", "main"])

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(doctor)).toContain("base_branch_missing")
  })

  // Branches under sprint/<name>/ look authoritative even when the CLI did not create them.
  // Flagging the current unrecorded branch prevents agents from doing workflow work there.
  test("reports current sprint namespace branches not recorded in state", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "-b", "sprint/example/experimental"])

    const result = await runCli(repo, ["doctor", "--sprint", "example", "--json"])
    const doctor = JSON.parse(result.stdout) as DoctorOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(doctor)).toContain("current_unrecorded_sprint_branch")
  })
})

type DoctorOutput = {
  ok: boolean
  nextSafeCommand?: string
  diagnostics: Array<{ code: string }>
}

function diagnosticCodes(doctor: DoctorOutput) {
  return doctor.diagnostics.map((diagnostic) => diagnostic.code)
}

async function gitAllowFailure(cwd: string, args: string[]) {
  const subprocess = Bun.spawn(["git", ...args], {
    cwd,
    stdout: "pipe",
    stderr: "pipe",
  })
  await Promise.all([
    new Response(subprocess.stdout).text(),
    new Response(subprocess.stderr).text(),
    subprocess.exited,
  ])
}
