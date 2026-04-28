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
      schemaVersion: 1,
      sprint: "example",
      baseBranch: "main",
      branches,
      tasks: {
        review: "010-task-name",
        next: null,
        approved: [],
        finishedUnreviewed: [],
      },
      activeStashes: [],
      lock: null,
      conflict: null,
    })

    expect(parsed.diagnostics).toEqual([])
    expect(parsed.state?.branches).toEqual(branches)
  })

  test("rejects unsupported schema versions", () => {
    const parsed = parseSprintState({
      schemaVersion: 2,
    })

    expect(parsed.state).toBeNull()
    expect(parsed.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
      "unsupported_schema_version",
    )
  })
})
