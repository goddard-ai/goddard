import {
  IpcClientError,
  type IpcClientErrorPayload,
  type IpcErrorDescriptorForCode,
  type IpcErrorDetails,
} from "@goddard-ai/ipc"

import { VscodeTaskIpcErrors, type VscodeTaskErrorCode } from "../schema.ts"

type VscodeTaskIpcErrorDescriptor<TCode extends VscodeTaskErrorCode> = IpcErrorDescriptorForCode<
  typeof VscodeTaskIpcErrors,
  TCode
>

export function createVscodeTaskIpcError<TCode extends VscodeTaskErrorCode>(
  code: TCode,
  ...[details]: undefined extends IpcErrorDetails<VscodeTaskIpcErrorDescriptor<TCode>>
    ? [details?: IpcErrorDetails<VscodeTaskIpcErrorDescriptor<TCode>>]
    : [details: IpcErrorDetails<VscodeTaskIpcErrorDescriptor<TCode>>]
) {
  const input = {
    code,
    ...(details === undefined ? {} : { details }),
  } as IpcClientErrorPayload<VscodeTaskIpcErrorDescriptor<TCode>>

  return new IpcClientError<VscodeTaskIpcErrorDescriptor<TCode>>(input)
}
