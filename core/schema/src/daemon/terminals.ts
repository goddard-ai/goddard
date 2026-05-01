/** Shared daemon terminal HTTP request and event-stream contract. */
import { z } from "zod"

/**
 * Daemon-minted id for the host terminal event stream that owns terminal instances.
 * Closing this stream disposes every terminal instance created under the connection.
 */
export const TerminalConnectionId = z.string().min(1)

export type TerminalConnectionId = z.infer<typeof TerminalConnectionId>

/** Connection-local id chosen by the terminal client for one daemon-managed terminal. */
export const TerminalInstanceId = z.string().min(1)

export type TerminalInstanceId = z.infer<typeof TerminalInstanceId>

/** PTY dimensions used when creating, resizing, or reporting one terminal runtime. */
export const TerminalDimensions = z.strictObject({
  cols: z.number().int().positive(),
  rows: z.number().int().positive(),
})

export type TerminalDimensions = z.infer<typeof TerminalDimensions>

/** Spawn options accepted when creating or restarting one daemon-managed terminal. */
export const TerminalSpawnOptions = z.strictObject({
  cwd: z.string().min(1).optional(),
  command: z.string().min(1).optional(),
  args: z.array(z.string()).optional(),
  env: z.record(z.string(), z.string()).optional(),
  title: z.string().optional(),
  dimensions: TerminalDimensions.optional(),
})

export type TerminalSpawnOptions = z.infer<typeof TerminalSpawnOptions>

/** Runtime states reported by the daemon for one connection-local terminal instance. */
export const TerminalRuntimeState = z.enum(["starting", "running", "exited", "closed", "error"])

export type TerminalRuntimeState = z.infer<typeof TerminalRuntimeState>

/** Runtime metadata returned for one terminal instance on the current terminal connection. */
export const TerminalRuntimeMetadata = z.strictObject({
  instanceId: TerminalInstanceId,
  state: TerminalRuntimeState,
  cwd: z.string().min(1).nullable(),
  title: z.string().nullable(),
  dimensions: TerminalDimensions,
  exitCode: z.number().int().nullable().optional(),
  signal: z.string().nullable().optional(),
})

export type TerminalRuntimeMetadata = z.infer<typeof TerminalRuntimeMetadata>

/** Request payload used to create one daemon terminal connection and event stream owner. */
export const TerminalConnectRequest = z.strictObject({})

export type TerminalConnectRequest = z.infer<typeof TerminalConnectRequest>

/** Response payload returned after creating one daemon terminal connection. */
export type TerminalConnectResponse = {
  connectionId: TerminalConnectionId
}

/** Path or filter params used to address one daemon terminal connection. */
export const TerminalConnectionParams = z.strictObject({
  connectionId: TerminalConnectionId,
})

export type TerminalConnectionParams = z.infer<typeof TerminalConnectionParams>

/** Stream filter used to subscribe to events for one daemon terminal connection. */
export const TerminalEventStreamFilter = TerminalConnectionParams

export type TerminalEventStreamFilter = z.infer<typeof TerminalEventStreamFilter>

/** Request payload fragment used to address one connection-local terminal instance. */
export const TerminalInstanceParams = TerminalConnectionParams.extend({
  instanceId: TerminalInstanceId,
})

export type TerminalInstanceParams = z.infer<typeof TerminalInstanceParams>

/** HTTP request payload that asks the daemon to create one terminal on a connection. */
export const TerminalCreateRequest = TerminalInstanceParams.extend({
  options: TerminalSpawnOptions.optional(),
})

export type TerminalCreateRequest = z.infer<typeof TerminalCreateRequest>

/** Response payload returned after creating one terminal instance. */
export type TerminalCreateResponse = {
  terminal: TerminalRuntimeMetadata
}

/** HTTP request payload that writes raw terminal input to one connection-local instance. */
export const TerminalInputRequest = TerminalInstanceParams.extend({
  data: z.string().min(1),
})

export type TerminalInputRequest = z.infer<typeof TerminalInputRequest>

/** HTTP request payload that resizes one connection-local terminal instance. */
export const TerminalResizeRequest = TerminalInstanceParams.extend({
  dimensions: TerminalDimensions,
})

export type TerminalResizeRequest = z.infer<typeof TerminalResizeRequest>

/** HTTP request payload that restarts one connection-local terminal instance. */
export const TerminalRestartRequest = TerminalInstanceParams.extend({
  options: TerminalSpawnOptions.optional(),
})

export type TerminalRestartRequest = z.infer<typeof TerminalRestartRequest>

/** HTTP request payload that disposes one connection-local terminal instance. */
export const TerminalCloseRequest = TerminalInstanceParams

export type TerminalCloseRequest = z.infer<typeof TerminalCloseRequest>

/** HTTP request payload that disposes all terminal instances for one connection. */
export const TerminalDisconnectRequest = TerminalConnectionParams

export type TerminalDisconnectRequest = z.infer<typeof TerminalDisconnectRequest>

/** Common event payload fields for one daemon terminal event stream. */
const TerminalConnectionEvent = z.strictObject({
  connectionId: TerminalConnectionId,
})

/** Common event payload fields for one connection-local terminal instance. */
const TerminalInstanceEvent = TerminalConnectionEvent.extend({
  instanceId: TerminalInstanceId,
})

/** Daemon stream event emitted after a terminal instance has been created on a connection. */
export const TerminalCreatedEvent = TerminalConnectionEvent.extend({
  type: z.literal("terminal.created"),
  terminal: TerminalRuntimeMetadata,
})

export type TerminalCreatedEvent = z.infer<typeof TerminalCreatedEvent>

/** Daemon stream event carrying terminal output for one connection-local instance. */
export const TerminalOutputEvent = TerminalInstanceEvent.extend({
  type: z.literal("terminal.output"),
  data: z.string().min(1),
})

export type TerminalOutputEvent = z.infer<typeof TerminalOutputEvent>

/** Daemon stream event emitted when one terminal process exits. */
export const TerminalExitEvent = TerminalInstanceEvent.extend({
  type: z.literal("terminal.exit"),
  exitCode: z.number().int().nullable(),
  signal: z.string().nullable(),
})

export type TerminalExitEvent = z.infer<typeof TerminalExitEvent>

/** Daemon stream event emitted when a terminal title changes. */
export const TerminalTitleEvent = TerminalInstanceEvent.extend({
  type: z.literal("terminal.title"),
  title: z.string(),
})

export type TerminalTitleEvent = z.infer<typeof TerminalTitleEvent>

/** Daemon stream event emitted when a terminal reports a current working directory change. */
export const TerminalCwdEvent = TerminalInstanceEvent.extend({
  type: z.literal("terminal.cwd"),
  cwd: z.string().min(1),
})

export type TerminalCwdEvent = z.infer<typeof TerminalCwdEvent>

/** Stable error categories for terminal request or stream failures. */
export const TerminalErrorCode = z.enum([
  "invalid-request",
  "duplicate-instance",
  "unknown-connection",
  "unknown-instance",
  "spawn-failed",
  "input-failed",
  "resize-failed",
  "restart-failed",
  "close-failed",
  "internal-error",
])

export type TerminalErrorCode = z.infer<typeof TerminalErrorCode>

/** Daemon stream event emitted for connection, instance, or terminal runtime failures. */
export const TerminalErrorEvent = TerminalConnectionEvent.extend({
  type: z.literal("terminal.error"),
  instanceId: TerminalInstanceId.optional(),
  code: TerminalErrorCode,
  message: z.string().min(1),
  recoverable: z.boolean(),
})

export type TerminalErrorEvent = z.infer<typeof TerminalErrorEvent>

/** All terminal events emitted by one daemon terminal event stream. */
export const TerminalDaemonEvent = z.discriminatedUnion("type", [
  TerminalCreatedEvent,
  TerminalOutputEvent,
  TerminalExitEvent,
  TerminalTitleEvent,
  TerminalCwdEvent,
  TerminalErrorEvent,
])

export type TerminalDaemonEvent = z.infer<typeof TerminalDaemonEvent>
