import type { z } from "zod"

export type IpcErrorDescriptor<
  TCode extends string = string,
  TDetails extends z.ZodType<unknown> = z.ZodType<unknown>,
> = {
  readonly code: TCode
  readonly details: TDetails
}

export type IpcErrorRegistry = Record<string, IpcErrorDescriptor>

export type IpcErrorDescriptorForCode<
  TRegistry extends IpcErrorRegistry,
  TCode extends TRegistry[keyof TRegistry]["code"],
> = Extract<TRegistry[keyof TRegistry], { readonly code: TCode }>

export type IpcErrorDetails<TDescriptor extends IpcErrorDescriptor> = z.output<
  TDescriptor["details"]
>

export type IpcErrorRegistryError<TRegistry extends IpcErrorRegistry> = {
  [K in keyof TRegistry]: IpcClientErrorPayload<TRegistry[K]>
}[keyof TRegistry]

export type IpcClientErrorPayload<TDescriptor extends IpcErrorDescriptor> =
  undefined extends IpcErrorDetails<TDescriptor>
    ? {
        code: TDescriptor["code"]
        details?: IpcErrorDetails<TDescriptor>
      }
    : {
        code: TDescriptor["code"]
        details: IpcErrorDetails<TDescriptor>
      }

type IpcClientErrorInput<TDescriptor extends IpcErrorDescriptor> =
  | string
  | IpcClientErrorPayload<TDescriptor>

/** Error whose message and optional structured code/details are safe to return to the IPC client. */
export class IpcClientError<
  TDescriptor extends IpcErrorDescriptor = IpcErrorDescriptor,
> extends Error {
  readonly code: TDescriptor["code"] | null
  readonly details: IpcErrorDetails<TDescriptor> | undefined

  constructor(input: IpcClientErrorInput<TDescriptor>, options?: ErrorOptions) {
    const message = typeof input === "string" ? input : `IPC request failed: ${input.code}`
    super(message, options)
    this.name = "IpcClientError"
    this.code = typeof input === "string" ? null : input.code
    this.details = typeof input === "string" ? undefined : input.details
  }
}
