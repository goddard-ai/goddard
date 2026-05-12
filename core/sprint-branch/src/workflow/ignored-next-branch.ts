import type { SprintBranchState } from "../types"

/** Checks whether finalization already accepted this exact dormant next divergence. */
export function isIgnoredNextBranchAtFinalize(
  state: SprintBranchState,
  reviewCommit: string | null,
  nextCommit: string | null,
) {
  return Boolean(
    state.ignoredNextBranchAtFinalize &&
    reviewCommit &&
    nextCommit &&
    state.ignoredNextBranchAtFinalize.reviewCommit === reviewCommit &&
    state.ignoredNextBranchAtFinalize.nextCommit === nextCommit,
  )
}

/** Records the dormant next divergence accepted by a successful finalization. */
export function recordIgnoredNextBranchAtFinalize(
  state: SprintBranchState,
  reviewCommit: string | null,
  nextCommit: string | null,
) {
  state.ignoredNextBranchAtFinalize =
    reviewCommit && nextCommit && reviewCommit !== nextCommit
      ? {
          reviewCommit,
          nextCommit,
        }
      : null
}
