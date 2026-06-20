type IpcClientErrorInput =
  | string
  | {
      code: string
      details?: unknown
      message: string
    }

/** Error whose message and optional structured code/details are safe to return to the IPC client. */
export class IpcClientError extends Error {
  readonly code: string | null
  readonly details: unknown

  constructor(input: IpcClientErrorInput, options?: ErrorOptions) {
    const message = typeof input === "string" ? input : input.message
    super(message, options)
    this.name = "IpcClientError"
    this.code = typeof input === "string" ? null : input.code
    this.details = typeof input === "string" ? undefined : input.details
  }
}
