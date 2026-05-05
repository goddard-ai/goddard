import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { repairLocalSprints } from "../scripts/repair-local-sprints"
import { parseReviewReport, reviewReportHeadings } from "../src/review-report"
import {
  cleanupTestRepos,
  commitAll,
  createBaseRepo,
  git,
  writeCompleteReviewReport,
} from "./support"

describe("local sprint repair script", () => {
  afterEach(cleanupTestRepos)

  test("removes obsolete sprint files, untracks sprints, and scaffolds missing report sections", async () => {
    const repo = await createBaseRepo("example")
    await fs.writeFile(path.join(repo, "sprints", "example", "000-index.md"), "# Index\n")
    await fs.writeFile(path.join(repo, "sprints", "example", "001-handoff.md"), "# Handoff\n")
    await fs.writeFile(
      path.join(repo, "sprints", "example", "020-task-name.md"),
      "# Task 020\n\n## Review Report\n\n### Plain-English Summary\n\nDone.\n",
    )
    await commitAll(repo, "add legacy sprint files")

    const report = await repairLocalSprints({ cwd: repo })
    const taskText = await fs.readFile(
      path.join(repo, "sprints", "example", "010-task-name.md"),
      "utf-8",
    )
    const partialTask = report.manualActions.find(
      (action) => action.relativePath === "sprints/example/020-task-name.md",
    )
    const newTask = report.manualActions.find((action) =>
      action.relativePath.endsWith("010-task-name.md"),
    )

    expect(report.ok).toBe(false)
    expect(report.fixes.gitExcludeAdded).toBe(true)
    expect(report.fixes.removedFromGitIndex).toEqual(
      expect.arrayContaining([
        "sprints/example/000-index.md",
        "sprints/example/001-handoff.md",
        "sprints/example/010-task-name.md",
        "sprints/example/020-task-name.md",
      ]),
    )
    expect(report.fixes.removedObsoleteFiles).toEqual([
      "sprints/example/000-index.md",
      "sprints/example/001-handoff.md",
    ])
    expect(await pathExists(path.join(repo, "sprints", "example", "000-index.md"))).toBe(false)
    expect(await pathExists(path.join(repo, "sprints", "example", "001-handoff.md"))).toBe(false)
    expect(await git(repo, ["ls-files", "--", "sprints"])).toBe("")
    expect(await fs.readFile(path.join(repo, ".git", "info", "exclude"), "utf-8")).toContain(
      "sprints/",
    )
    expect(taskText).toContain("## Review Report")
    for (const heading of reviewReportHeadings) {
      expect(taskText).toContain(`### ${heading}`)
    }
    expect(newTask?.sections).toEqual(reviewReportHeadings)
    expect(partialTask?.sections).toEqual(reviewReportHeadings.slice(1))
    expect(parseReviewReport(taskText, "010-task-name").diagnostics).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          code: "review_report_incomplete",
        }),
      ]),
    )
  })

  test("leaves complete Review Reports unchanged while still untracking sprint files", async () => {
    const repo = await createBaseRepo("example")
    await writeCompleteReviewReport(repo, "example", "010-task-name")
    await writeCompleteReviewReport(repo, "example", "020-task-name")
    await commitAll(repo, "add complete review reports")
    const before = await fs.readFile(
      path.join(repo, "sprints", "example", "010-task-name.md"),
      "utf-8",
    )

    const report = await repairLocalSprints({ cwd: repo })
    const after = await fs.readFile(
      path.join(repo, "sprints", "example", "010-task-name.md"),
      "utf-8",
    )

    expect(report.ok).toBe(true)
    expect(report.manualActions).toEqual([])
    expect(report.fixes.updatedTaskFiles).toEqual([])
    expect(after).toBe(before)
    expect(await git(repo, ["ls-files", "--", "sprints"])).toBe("")
  })

  test("dry-run reports fixes without mutating local sprint artifacts", async () => {
    const repo = await createBaseRepo("example")
    await fs.writeFile(path.join(repo, "sprints", "example", "000-index.md"), "# Index\n")
    await commitAll(repo, "add legacy index")

    const report = await repairLocalSprints({ cwd: repo, dryRun: true })

    expect(report.dryRun).toBe(true)
    expect(report.fixes.gitExcludeAdded).toBe(true)
    expect(report.fixes.removedObsoleteFiles).toEqual(["sprints/example/000-index.md"])
    expect(await pathExists(path.join(repo, "sprints", "example", "000-index.md"))).toBe(true)
    expect(await git(repo, ["ls-files", "--", "sprints"])).toContain("sprints/example/000-index.md")
    expect(
      await fs.readFile(path.join(repo, "sprints", "example", "010-task-name.md"), "utf-8"),
    ).not.toContain("## Review Report")
  })
})

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      (error as { code?: unknown }).code === "ENOENT"
    ) {
      return false
    }
    throw error
  }
}
