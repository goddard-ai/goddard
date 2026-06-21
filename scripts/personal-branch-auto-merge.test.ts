import { describe, expect, test } from "bun:test"

import {
  summarizeMergeGate,
  summarizeReviewState,
  summarizeStatusChecks,
} from "./personal-branch-auto-merge.ts"

const passingCheckRun = {
  __typename: "CheckRun",
  conclusion: "SUCCESS",
  name: "Test",
  status: "COMPLETED",
} as const

const passingStatusContext = {
  __typename: "StatusContext",
  context: "legacy-ci",
  state: "SUCCESS",
} as const

const resolvedReviews = {
  reviewDecision: null,
  reviewThreads: [{ isResolved: true }],
}

describe("summarizeStatusChecks", () => {
  test("blocks when no checks have reported", () => {
    expect(summarizeStatusChecks([])).toBe("NO_CHECKS")
  })

  test("accepts passing check runs and status contexts", () => {
    expect(summarizeStatusChecks([passingCheckRun, passingStatusContext])).toBe("PASSING")
  })

  test("blocks incomplete, failing, and unknown check records", () => {
    expect(
      summarizeStatusChecks([{ ...passingCheckRun, conclusion: null, status: "IN_PROGRESS" }]),
    ).toBe("NOT_PASSING")

    expect(summarizeStatusChecks([{ ...passingCheckRun, conclusion: "FAILURE" }])).toBe(
      "NOT_PASSING",
    )

    expect(summarizeStatusChecks([{ __typename: "UnexpectedCheck" }])).toBe("NOT_PASSING")
  })
})

describe("summarizeReviewState", () => {
  test("blocks requested changes", () => {
    expect(
      summarizeReviewState({
        reviewDecision: "CHANGES_REQUESTED",
        reviewThreads: [{ isResolved: true }],
      }),
    ).toBe("CHANGES_REQUESTED")
  })

  test("blocks unresolved review threads", () => {
    expect(
      summarizeReviewState({
        reviewDecision: "APPROVED",
        reviewThreads: [{ isResolved: true }, { isResolved: false }],
      }),
    ).toBe("UNRESOLVED_THREADS")
  })

  test("passes when review feedback is resolved", () => {
    expect(summarizeReviewState(resolvedReviews)).toBe("RESOLVED")
  })
})

describe("summarizeMergeGate", () => {
  test("blocks draft or unclean PRs before checking merge policy details", () => {
    expect(
      summarizeMergeGate(
        {
          isDraft: true,
          mergeStateStatus: "CLEAN",
          statusCheckRollup: [passingCheckRun],
        },
        resolvedReviews,
      ),
    ).toEqual({ detail: "mergeStateStatus=DRAFT", state: "BLOCKED" })

    expect(
      summarizeMergeGate(
        {
          isDraft: false,
          mergeStateStatus: "BLOCKED",
          statusCheckRollup: [passingCheckRun],
        },
        resolvedReviews,
      ),
    ).toEqual({ detail: "mergeStateStatus=BLOCKED", state: "BLOCKED" })
  })

  test("blocks PRs without passing checks or resolved review feedback", () => {
    expect(
      summarizeMergeGate(
        {
          isDraft: false,
          mergeStateStatus: "CLEAN",
          statusCheckRollup: [],
        },
        resolvedReviews,
      ),
    ).toEqual({ detail: "status checks are NO_CHECKS", state: "BLOCKED" })

    expect(
      summarizeMergeGate(
        {
          isDraft: false,
          mergeStateStatus: "CLEAN",
          statusCheckRollup: [passingCheckRun],
        },
        {
          reviewDecision: "APPROVED",
          reviewThreads: [{ isResolved: false }],
        },
      ),
    ).toEqual({ detail: "review feedback is UNRESOLVED_THREADS", state: "BLOCKED" })
  })

  test("passes only when mergeability, checks, and reviews are ready", () => {
    expect(
      summarizeMergeGate(
        {
          isDraft: false,
          mergeStateStatus: "CLEAN",
          statusCheckRollup: [passingCheckRun, passingStatusContext],
        },
        resolvedReviews,
      ),
    ).toEqual({ state: "READY" })
  })
})
