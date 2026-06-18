import { describe, expect, test } from "bun:test"

import {
  findRebaseCheckedBranchPushSha,
  getTurboAffectedFilterRange,
  parsePushUpdates,
} from "./pre-push.ts"

const PUSHED_SHA = "1111111111111111111111111111111111111111"
const REMOTE_SHA = "2222222222222222222222222222222222222222"
const ZERO_SHA = "0000000000000000000000000000000000000000"

describe("findRebaseCheckedBranchPushSha", () => {
  test("selects pushes to the personal branch on origin", () => {
    const updates = parsePushUpdates(
      `refs/heads/aleclarson ${PUSHED_SHA} refs/heads/aleclarson ${REMOTE_SHA}\n`,
    )

    expect(findRebaseCheckedBranchPushSha("origin", updates)).toBe(PUSHED_SHA)
  })

  test("ignores deletes, non-origin remotes, and unrelated branches", () => {
    expect(
      findRebaseCheckedBranchPushSha(
        "origin",
        parsePushUpdates(`(delete) ${ZERO_SHA} refs/heads/aleclarson ${REMOTE_SHA}\n`),
      ),
    ).toBeUndefined()

    expect(
      findRebaseCheckedBranchPushSha(
        "upstream",
        parsePushUpdates(
          `refs/heads/aleclarson ${PUSHED_SHA} refs/heads/aleclarson ${REMOTE_SHA}\n`,
        ),
      ),
    ).toBeUndefined()

    expect(
      findRebaseCheckedBranchPushSha(
        "origin",
        parsePushUpdates(`refs/heads/topic ${PUSHED_SHA} refs/heads/topic ${REMOTE_SHA}\n`),
      ),
    ).toBeUndefined()
  })
})

describe("getTurboAffectedFilterRange", () => {
  test("uses the pushed remote-to-local range for existing remote refs", () => {
    const [update] = parsePushUpdates(
      `refs/heads/topic ${PUSHED_SHA} refs/heads/topic ${REMOTE_SHA}\n`,
    )

    expect(getTurboAffectedFilterRange(update!)).toBe(`${REMOTE_SHA}...${PUSHED_SHA}`)
  })

  test("uses the branch point for new remote refs", () => {
    const branchPoint = "3333333333333333333333333333333333333333"
    const [update] = parsePushUpdates(
      `refs/heads/topic ${PUSHED_SHA} refs/heads/topic ${ZERO_SHA}\n`,
    )

    expect(getTurboAffectedFilterRange(update!, branchPoint)).toBe(`${branchPoint}...${PUSHED_SHA}`)
    expect(getTurboAffectedFilterRange(update!)).toBeUndefined()
  })
})
