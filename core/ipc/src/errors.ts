/** Error whose message is safe to return to the IPC client unchanged. */
export class IpcClientError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = "IpcClientError"
  }
}
