import { IpcClientError } from "@goddard-ai/ipc"

import type { SessionErrorCode } from "../schema.ts"

export function createSessionIpcError(
  code: SessionErrorCode,
  message: string,
  details?: Record<string, unknown>,
) {
  return new IpcClientError({
    code,
    ...(details ? { details } : {}),
    message,
  })
}
