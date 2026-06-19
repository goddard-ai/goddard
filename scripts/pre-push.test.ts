import { describe, expect, test } from "bun:test"

import {
  findRebaseCheckedBranchPushSha,
  getTurboAffectedFilterRange,
  parsePushUpdates,
  shouldRunBunRuntimeCheck,
  shouldRunDocsCheck,
  shouldRunRepoCheck,
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

describe("changed file checks", () => {
  test("detects files that require full repo checks", () => {
    expect(shouldRunRepoCheck(["features/session/src/sdk.ts"])).toBe(true)
    expect(shouldRunRepoCheck(["README.md"])).toBe(false)
  })

  test("detects files that require Bun runtime checks", () => {
    expect(shouldRunBunRuntimeCheck(["package.json"])).toBe(true)
    expect(shouldRunBunRuntimeCheck(["pnpm-lock.yaml"])).toBe(true)
    expect(shouldRunBunRuntimeCheck(["app/electrobun.config.ts"])).toBe(true)
    expect(shouldRunBunRuntimeCheck(["app/src/bun/index.ts"])).toBe(false)
  })

  test("detects public docs changes without legacy technical docs", () => {
    expect(shouldRunDocsCheck(["core/daemon/docs/README.md"])).toBe(true)
    expect(shouldRunDocsCheck(["core/daemon/docs/assets/diagram.png"])).toBe(true)
    expect(shouldRunDocsCheck(["core/schema/docs/session-titles-and-model-selection.md"])).toBe(
      false,
    )
    expect(shouldRunDocsCheck(["README.md"])).toBe(false)
  })
})
