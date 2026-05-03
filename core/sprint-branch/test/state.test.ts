import { describe, expect, test } from "bun:test"

import {
  getExpectedBranches,
  parseSprintBranchName,
  parseSprintState,
  validateSprintName,
} from "../src"

describe("sprint branch state parsing", () => {
  test("validates sprint names as single path segments", () => {
    expect(validateSprintName("example")).toEqual([])
    expect(validateSprintName("bad/name").map((diagnostic) => diagnostic.code)).toContain(
      "invalid_sprint_path_segment",
    )
    expect(validateSprintName("bad name").map((diagnostic) => diagnostic.code)).toContain(
      "invalid_sprint_whitespace",
    )
  })

  test("parses canonical sprint branch names", () => {
    expect(parseSprintBranchName("sprint/example/review")).toEqual({
      sprint: "example",
      role: "review",
    })
    expect(parseSprintBranchName("feature/example")).toBeNull()
  })

  test("accepts the canonical schema", () => {
    const branches = getExpectedBranches("example")
    const parsed = parseSprintState({
      sprint: "example",
      baseBranch: "main",
      visibility: "parked",
      tasks: {
        review: "010-task-name",
        next: null,
        approved: [],
        finishedUnreviewed: [],
      },
      activeStashes: [],
      conflict: null,
    })

    expect(parsed.diagnostics).toEqual([])
    expect(parsed.state?.branches).toEqual(branches)
    expect(parsed.state?.visibility).toBe("parked")
    expect(parsed.state?.tasks.finishedUnreviewed).toEqual([])
  })

  test("defaults missing visibility to active", () => {
    const parsed = parseSprintState({
      sprint: "example",
      baseBranch: "main",
      tasks: {
        review: "010-task-name",
        next: null,
        approved: [],
      },
      activeStashes: [],
      conflict: null,
    })

    expect(parsed.diagnostics).toEqual([])
    expect(parsed.state?.visibility).toBe("active")
    expect(parsed.state?.tasks.finishedUnreviewed).toEqual([])
  })

  test("rejects invalid task state", () => {
    const parsed = parseSprintState({
      sprint: "example",
      baseBranch: "main",
      tasks: {
        review: "010-task-name",
        next: null,
        approved: null,
      },
      activeStashes: [],
      conflict: null,
    })

    expect(parsed.state).toBeNull()
    expect(parsed.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
      "invalid_string_array",
    )
  })

  test("rejects invalid visibility", () => {
    const parsed = parseSprintState({
      sprint: "example",
      baseBranch: "main",
      visibility: "hidden",
      tasks: {
        review: "010-task-name",
        next: null,
        approved: [],
      },
      activeStashes: [],
      conflict: null,
    })

    expect(parsed.state).toBeNull()
    expect(parsed.diagnostics.map((diagnostic) => diagnostic.code)).toContain("invalid_visibility")
  })
})
