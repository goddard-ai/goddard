/**
 * SDK-facing terminal connection surface for daemon-managed terminal instances.
 *
 * A terminal connection is intentionally websocket-scoped: every instance id is local to the
 * connection, and disconnecting the desktop host terminal connection disposes all instances it
 * created. The app webview should interact with terminal state through its Bun host, not by
 * owning this daemon websocket directly.
 */
import type {
  TerminalCloseRequest,
  TerminalCreateRequest,
  TerminalDaemonEvent,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
} from "@goddard-ai/schema/daemon/terminals"

/** SDK input for creating one terminal without exposing the raw frame discriminator. */
export type TerminalCreateInput = Omit<TerminalCreateRequest, "type">

/** SDK input for writing terminal data without exposing the raw frame discriminator. */
export type TerminalWriteInput = Omit<TerminalInputRequest, "type">

/** SDK input for resizing one terminal without exposing the raw frame discriminator. */
export type TerminalResizeInput = Omit<TerminalResizeRequest, "type">

/** SDK input for restarting one terminal without exposing the raw frame discriminator. */
export type TerminalRestartInput = Omit<TerminalRestartRequest, "type">

/** SDK input for closing one terminal without exposing the raw frame discriminator. */
export type TerminalCloseInput = Omit<TerminalCloseRequest, "type">

/** Callback shape used by hosts to receive daemon terminal lifecycle events. */
export type TerminalEventHandler = (event: TerminalDaemonEvent) => void

/** Options for opening one daemon terminal websocket through the SDK surface. */
export type TerminalConnectOptions = {
  onEvent?: TerminalEventHandler
  signal?: AbortSignal
}

/** One SDK terminal connection that owns connection-local terminal instances. */
export interface GoddardTerminalConnection {
  create(input: TerminalCreateInput): Promise<void>
  write(input: TerminalWriteInput): Promise<void>
  resize(input: TerminalResizeInput): Promise<void>
  restart(input: TerminalRestartInput): Promise<void>
  close(input: TerminalCloseInput): Promise<void>
  disconnect(): Promise<void>
  onEvent(handler: TerminalEventHandler): () => void
}

/** SDK namespace shape that future runtime work will expose as `sdk.terminal`. */
export type GoddardTerminalNamespace = {
  connect(options?: TerminalConnectOptions): Promise<GoddardTerminalConnection>
}
