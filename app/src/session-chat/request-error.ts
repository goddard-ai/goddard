import { IpcClientError } from "@goddard-ai/ipc"
import { SessionErrorCodes } from "@goddard-ai/sdk"
import { t } from "@lingui/core/macro"

export function formatSessionRequestError(error: unknown) {
  if (error instanceof IpcClientError && error.code === SessionErrorCodes.NotActive) {
    return t`This session is no longer active. Refresh the session and try again.`
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return t`The daemon rejected the request.`
}
