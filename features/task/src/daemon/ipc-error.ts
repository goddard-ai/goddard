import {
  IpcClientError,
  type IpcClientErrorPayload,
  type IpcErrorDescriptorForCode,
  type IpcErrorDetails,
} from "@goddard-ai/ipc"

import { TaskIpcErrors, type TaskErrorCode } from "../schema.ts"

type TaskIpcErrorDescriptor<TCode extends TaskErrorCode> = IpcErrorDescriptorForCode<
  typeof TaskIpcErrors,
  TCode
>

export function createTaskIpcError<TCode extends TaskErrorCode>(
  code: TCode,
  ...[details]: undefined extends IpcErrorDetails<TaskIpcErrorDescriptor<TCode>>
    ? [details?: IpcErrorDetails<TaskIpcErrorDescriptor<TCode>>]
    : [details: IpcErrorDetails<TaskIpcErrorDescriptor<TCode>>]
) {
  const input = {
    code,
    ...(details === undefined ? {} : { details }),
  } as IpcClientErrorPayload<TaskIpcErrorDescriptor<TCode>>

  return new IpcClientError<TaskIpcErrorDescriptor<TCode>>(input)
}
