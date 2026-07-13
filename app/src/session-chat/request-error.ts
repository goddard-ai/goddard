import {
  assertNever,
  isIpcClientErrorForRegistry,
  type IpcClientErrorForRegistry,
} from "@goddard-ai/ipc"
import { SessionErrorCodes, SessionIpcErrors } from "@goddard-ai/sdk"
import { t } from "@lingui/core/macro"

export function formatSessionRequestError(error: unknown) {
  if (isIpcClientErrorForRegistry(error, SessionIpcErrors)) {
    return formatKnownSessionIpcError(error)
  }

  return t`The request failed.`
}

function formatKnownSessionIpcError(error: IpcClientErrorForRegistry<typeof SessionIpcErrors>) {
  switch (error.code) {
    case SessionErrorCodes.ArchivedNotReconnectable:
    case SessionErrorCodes.NotReconnectable:
      return t`This session can no longer be reconnected.`
    case SessionErrorCodes.CannotCompleteActiveTurn:
      return t`Wait for the active turn to finish before completing this session.`
    case SessionErrorCodes.CannotCompleteDirtyWorktree:
      return t`Commit or stash worktree changes before completing this session.`
    case SessionErrorCodes.CannotCompleteUnmergedCommits:
      return t`Merge the worktree commits before completing this session.`
    case SessionErrorCodes.CannotInspectCompletionState:
      return t`The worktree state could not be inspected. Try again after checking the project.`
    case SessionErrorCodes.CannotResumeUnsupportedAgent:
      return t`This agent cannot reconnect to the previous session.`
    case SessionErrorCodes.InvalidCursor:
    case SessionErrorCodes.InvalidHistoryCursor:
      return t`The session data is out of date. Refresh and try again.`
    case SessionErrorCodes.InvalidToken:
      return t`This session link is no longer valid.`
    case SessionErrorCodes.LaunchBareRepository:
      return t`Start the session from a non-bare repository checkout.`
    case SessionErrorCodes.LaunchCheckoutFailed:
      return t`The branch checkout failed. Check the project and try again.`
    case SessionErrorCodes.LaunchDirtyCheckout:
      return t`Commit or stash local changes before starting this branch session.`
    case SessionErrorCodes.LaunchOutsideRepository:
      return t`Start the branch session from a repository checkout.`
    case SessionErrorCodes.MissingJsonRpcId:
    case SessionErrorCodes.UnsupportedMessage:
      return t`The session request is not supported.`
    case SessionErrorCodes.NotActive:
      return t`This session is no longer active. Refresh the session and try again.`
    case SessionErrorCodes.NotFound:
      return t`Session not found. Refresh and try again.`
    case SessionErrorCodes.NoWorktree:
      return t`This session does not have a worktree.`
    case SessionErrorCodes.PromptAborted:
      return t`The queued prompt was cancelled.`
    case SessionErrorCodes.ProfileConfigurationFailed:
      return t`Session profiles could not be saved. Check the configuration and try again.`
    default:
      return assertNever(error)
  }
}
