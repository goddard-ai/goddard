/**
 * SDK-facing terminal surface for daemon-managed terminal instances.
 *
 * Terminal control is modeled as daemon HTTP/IPC requests plus a daemon event stream. A terminal
 * connection id identifies the host-owned stream whose disconnect disposes every instance created
 * through that connection. The app webview should still interact with terminal state through its
 * Bun host rather than owning daemon terminal lifecycle directly.
 */
import type {
  TerminalCloseRequest,
  TerminalConnectRequest,
  TerminalConnectResponse,
  TerminalCreateRequest,
  TerminalCreateResponse,
  TerminalDaemonEvent,
  TerminalDisconnectRequest,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
} from "@goddard-ai/schema/daemon/terminals"

/** Callback shape used by hosts to receive daemon terminal lifecycle events. */
export type TerminalEventHandler = (event: TerminalDaemonEvent) => void

/** SDK input for writing terminal data through the request surface. */
export type TerminalWriteInput = TerminalInputRequest

/** One SDK terminal connection that owns connection-local terminal instances. */
export interface GoddardTerminalConnection {
  readonly connectionId: TerminalConnectResponse["connectionId"]
  create(input: Omit<TerminalCreateRequest, "connectionId">): Promise<TerminalCreateResponse>
  write(input: Omit<TerminalInputRequest, "connectionId">): Promise<void>
  resize(input: Omit<TerminalResizeRequest, "connectionId">): Promise<void>
  restart(input: Omit<TerminalRestartRequest, "connectionId">): Promise<TerminalCreateResponse>
  close(input: Omit<TerminalCloseRequest, "connectionId">): Promise<void>
  disconnect(): Promise<void>
  subscribe(handler: TerminalEventHandler): Promise<() => void>
}

/** SDK namespace shape that future runtime work will expose as `sdk.terminal`. */
export type GoddardTerminalNamespace = {
  connect(input?: TerminalConnectRequest): Promise<GoddardTerminalConnection>
  create(input: TerminalCreateRequest): Promise<TerminalCreateResponse>
  write(input: TerminalWriteInput): Promise<void>
  resize(input: TerminalResizeRequest): Promise<void>
  restart(input: TerminalRestartRequest): Promise<TerminalCreateResponse>
  close(input: TerminalCloseRequest): Promise<void>
  disconnect(input: TerminalDisconnectRequest): Promise<void>
  subscribe(input: TerminalDisconnectRequest, handler: TerminalEventHandler): Promise<() => void>
}
