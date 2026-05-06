import { createRequire } from "node:module"

import type { SprintBranchState } from "./types"

const require = createRequire(import.meta.url)

type NodeNotifier = {
  notify: (
    options: {
      title: string
      subtitle?: string
      message: string
      sound?: boolean | string
    },
    callback?: (error?: unknown) => void,
  ) => void
}

/** Payload sent when the sprint review branch first becomes ready for a human. */
export type ReviewReadyNotification = {
  sprint: string
  task: string
  reviewBranch: string
}

/** Adapter for the desktop notification side effect. */
export type ReviewReadyNotifier = (notification: ReviewReadyNotification) => void | Promise<void>

/** Sends a best-effort alert when a mutation makes a new review task ready. */
export async function notifyReviewBranchReadyTransition(input: {
  before: SprintBranchState
  after: SprintBranchState
  notifier?: ReviewReadyNotifier
}) {
  const beforeTask = reviewReadyTask(input.before)
  const afterTask = reviewReadyTask(input.after)

  if (!afterTask || beforeTask === afterTask) {
    return
  }

  try {
    await (input.notifier ?? notifyReviewBranchReady)({
      sprint: input.after.sprint,
      task: afterTask,
      reviewBranch: input.after.branches.review,
    })
  } catch {
    // Review readiness is already recorded; notification delivery is best-effort.
  }
}

/** Uses node-notifier to alert the user that a sprint branch is ready to review. */
export async function notifyReviewBranchReady(notification: ReviewReadyNotification) {
  if (process.env.SPRINT_BRANCH_DISABLE_NOTIFICATIONS === "1") {
    return
  }

  await new Promise<void>((resolve, reject) => {
    const notifier = require("node-notifier") as NodeNotifier
    notifier.notify(
      {
        title: "Sprint branch ready for review",
        subtitle: notification.sprint,
        message: `${notification.task} is ready on ${notification.reviewBranch}.`,
        sound: process.platform === "darwin" ? "Hero" : true,
      },
      (error) => {
        if (error) {
          reject(error)
          return
        }
        resolve()
      },
    )
  })
}

function reviewReadyTask(state: SprintBranchState) {
  return state.tasks.review && state.tasks.finishedUnreviewed.includes(state.tasks.review)
    ? state.tasks.review
    : null
}
