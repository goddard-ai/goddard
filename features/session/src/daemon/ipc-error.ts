import {
  IpcClientError,
  type IpcClientErrorPayload,
  type IpcErrorDescriptorForCode,
  type IpcErrorDetails,
} from "@goddard-ai/ipc"

import { SessionIpcErrors, type SessionErrorCode } from "../schema.ts"

type SessionIpcErrorDescriptor<TCode extends SessionErrorCode> = IpcErrorDescriptorForCode<
  typeof SessionIpcErrors,
  TCode
>

export function createSessionIpcError<TCode extends SessionErrorCode>(
  code: TCode,
  ...[details]: undefined extends IpcErrorDetails<SessionIpcErrorDescriptor<TCode>>
    ? [details?: IpcErrorDetails<SessionIpcErrorDescriptor<TCode>>]
    : [details: IpcErrorDetails<SessionIpcErrorDescriptor<TCode>>]
) {
  const input = {
    code,
    ...(details === undefined ? {} : { details }),
  } as IpcClientErrorPayload<SessionIpcErrorDescriptor<TCode>>

  return new IpcClientError<SessionIpcErrorDescriptor<TCode>>(input)
}
