import { afterEach, describe, expect, mock, test } from "bun:test"

import type { SprintDiagnostic } from "../src/types"
import { cleanupTestRepos, createSprintRepo, readState, writeState } from "./support"

let selectedSprints: string[] = []

mock.module("@clack/prompts", () => ({
  autocomplete: async () => selectedSprints[0],
  autocompleteMultiselect: async () => selectedSprints,
  confirm: async () => true,
  isCancel: (value: unknown) => typeof value === "symbol",
}))

const { resolveCleanupCandidates } = await import("../src/landing/selection")

describe("cleanup sprint selection", () => {
  afterEach(async () => {
    selectedSprints = []
    await cleanupTestRepos()
  })

  test("allows selecting multiple sprints from the cleanup prompt", async () => {
    const repo = await createSprintRepo("alpha", {
      review: null,
      next: null,
      approved: ["010-task-name"],
    })
    const alpha = await readState(repo, "alpha")
    await writeState(repo, "beta", {
      ...alpha,
      sprint: "beta",
      branches: {
        approved: "sprint/beta/approved",
        review: "sprint/beta/review",
        next: "sprint/beta/next",
      },
    })
    selectedSprints = ["alpha", "beta"]
    const diagnostics: SprintDiagnostic[] = []
    const restoreTty = forceTty()

    try {
      const candidates = await resolveCleanupCandidates(
        repo,
        { cwd: repo, dryRun: false, json: false },
        null,
        diagnostics,
      )

      expect(candidates.map((candidate) => candidate.sprint)).toEqual(["alpha", "beta"])
      expect(diagnostics).toEqual([])
    } finally {
      restoreTty()
    }
  })
})

function forceTty() {
  const stdinDescriptor = Object.getOwnPropertyDescriptor(process.stdin, "isTTY")
  const stdoutDescriptor = Object.getOwnPropertyDescriptor(process.stdout, "isTTY")
  Object.defineProperty(process.stdin, "isTTY", { configurable: true, value: true })
  Object.defineProperty(process.stdout, "isTTY", { configurable: true, value: true })

  return () => {
    restoreProperty(process.stdin, "isTTY", stdinDescriptor)
    restoreProperty(process.stdout, "isTTY", stdoutDescriptor)
  }
}

function restoreProperty(
  object: NodeJS.ReadStream | NodeJS.WriteStream,
  property: "isTTY",
  descriptor: PropertyDescriptor | undefined,
) {
  if (descriptor) {
    Object.defineProperty(object, property, descriptor)
    return
  }
  Reflect.deleteProperty(object, property)
}
