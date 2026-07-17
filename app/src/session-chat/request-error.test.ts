import { IpcClientError } from "@goddard-ai/ipc"
import { SessionErrorCodes } from "@goddard-ai/sdk"
import { expect, test } from "vitest"

import { formatSessionRequestError } from "./request-error.ts"

const sessionId = "ses_1"

const sessionErrorCases = [
  [
    SessionErrorCodes.ArchivedNotReconnectable,
    { connectionMode: "history", sessionId },
    "This session can no longer be reconnected.",
  ],
  [
    SessionErrorCodes.CannotCompleteActiveTurn,
    { sessionId },
    "Wait for the active turn to finish before completing this session.",
  ],
  [
    SessionErrorCodes.CannotCompleteDirtyWorktree,
    { sessionId, worktreeDir: "/tmp/session-worktree" },
    "Commit or stash worktree changes before completing this session.",
  ],
  [
    SessionErrorCodes.CannotCompleteUnmergedCommits,
    { sessionId, worktreeDir: "/tmp/session-worktree" },
    "Merge the worktree commits before completing this session.",
  ],
  [
    SessionErrorCodes.CannotInspectCompletionState,
    { sessionId },
    "The worktree state could not be inspected. Try again after checking the project.",
  ],
  [
    SessionErrorCodes.CannotResumeUnsupportedAgent,
    { acpSessionId: "acp-session-1" },
    "This agent cannot reconnect to the previous session.",
  ],
  [
    SessionErrorCodes.InvalidCursor,
    { cursor: "cursor-1" },
    "The session data is out of date. Refresh and try again.",
  ],
  [
    SessionErrorCodes.InvalidHistoryCursor,
    { cursor: "cursor-1", sessionId },
    "The session data is out of date. Refresh and try again.",
  ],
  [SessionErrorCodes.InvalidToken, undefined, "This session link is no longer valid."],
  [
    SessionErrorCodes.LaunchBareRepository,
    { cwd: "/tmp/project" },
    "Start the session from a non-bare repository checkout.",
  ],
  [
    SessionErrorCodes.LaunchCheckoutFailed,
    { branchName: "feature", cwd: "/tmp/project", repoRoot: "/tmp/project" },
    "The branch checkout failed. Check the project and try again.",
  ],
  [
    SessionErrorCodes.LaunchDirtyCheckout,
    { cwd: "/tmp/project", repoRoot: "/tmp/project" },
    "Commit or stash local changes before starting this branch session.",
  ],
  [
    SessionErrorCodes.LaunchOutsideRepository,
    { cwd: "/tmp/project" },
    "Start the branch session from a repository checkout.",
  ],
  [SessionErrorCodes.MissingJsonRpcId, { sessionId }, "The session request is not supported."],
  [
    SessionErrorCodes.NotActive,
    { sessionId },
    "This session is no longer active. Refresh the session and try again.",
  ],
  [SessionErrorCodes.NotFound, { sessionId }, "Session not found. Refresh and try again."],
  [
    SessionErrorCodes.NotReconnectable,
    { connectionMode: "none", sessionId },
    "This session can no longer be reconnected.",
  ],
  [SessionErrorCodes.NoWorktree, { sessionId }, "This session does not have a worktree."],
  [SessionErrorCodes.PromptAborted, { sessionId }, "The queued prompt was cancelled."],
  [
    SessionErrorCodes.ProfileConfigurationFailed,
    undefined,
    "Session profiles could not be saved. Check the configuration and try again.",
  ],
  [SessionErrorCodes.UnsupportedMessage, { sessionId }, "The session request is not supported."],
] as const

test.each(sessionErrorCases)(
  "formats known session IPC error %s by exported code",
  (code, details, expected) => {
    expect(
      formatSessionRequestError(
        new IpcClientError({
          code,
          ...(details === undefined ? {} : { details }),
        }),
      ),
    ).toBe(expected)
  },
)

test("formats unknown IPC errors with a generic message", () => {
  expect(
    formatSessionRequestError(
      new IpcClientError({
        code: "session.future_error",
        details: { sessionId },
      }),
    ),
  ).toBe("The request failed.")
})

test("formats non-IPC errors with a generic message", () => {
  expect(formatSessionRequestError(new Error("Daemon rejected the request."))).toBe(
    "The request failed.",
  )
})

test("formats non-error rejections with a generic message", () => {
  expect(formatSessionRequestError(null)).toBe("The request failed.")
})
