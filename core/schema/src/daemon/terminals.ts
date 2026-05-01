/**
 * Shared daemon terminal websocket contract.
 *
 * Terminal instance ids are scoped to one terminal websocket connection. The daemon must not
 * persist them or route commands by a global terminal id; closing the websocket disposes every
 * instance created through that connection.
 */
import { z } from "zod"

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

/** Runtime metadata returned for one terminal instance on the current websocket connection. */
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

const TerminalInstanceFrame = z.strictObject({
  instanceId: TerminalInstanceId,
})

/** Client frame that asks the daemon to create one terminal on the current connection. */
export const TerminalCreateRequest = TerminalInstanceFrame.extend({
  type: z.literal("terminal.create"),
  options: TerminalSpawnOptions.optional(),
})

export type TerminalCreateRequest = z.infer<typeof TerminalCreateRequest>

/** Client frame that writes raw terminal input to one connection-local instance. */
export const TerminalInputRequest = TerminalInstanceFrame.extend({
  type: z.literal("terminal.input"),
  data: z.string().min(1),
})

export type TerminalInputRequest = z.infer<typeof TerminalInputRequest>

/** Client frame that resizes one connection-local terminal instance. */
export const TerminalResizeRequest = TerminalInstanceFrame.extend({
  type: z.literal("terminal.resize"),
  dimensions: TerminalDimensions,
})

export type TerminalResizeRequest = z.infer<typeof TerminalResizeRequest>

/** Client frame that restarts one connection-local terminal instance. */
export const TerminalRestartRequest = TerminalInstanceFrame.extend({
  type: z.literal("terminal.restart"),
  options: TerminalSpawnOptions.optional(),
})

export type TerminalRestartRequest = z.infer<typeof TerminalRestartRequest>

/** Client frame that disposes one connection-local terminal instance. */
export const TerminalCloseRequest = TerminalInstanceFrame.extend({
  type: z.literal("terminal.close"),
})

export type TerminalCloseRequest = z.infer<typeof TerminalCloseRequest>

/** All terminal control frames accepted from one daemon terminal websocket client. */
export const TerminalClientFrame = z.discriminatedUnion("type", [
  TerminalCreateRequest,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
  TerminalCloseRequest,
])

export type TerminalClientFrame = z.infer<typeof TerminalClientFrame>

/** Daemon event emitted after a terminal instance has been created on the current connection. */
export const TerminalCreatedEvent = z.strictObject({
  type: z.literal("terminal.created"),
  terminal: TerminalRuntimeMetadata,
})

export type TerminalCreatedEvent = z.infer<typeof TerminalCreatedEvent>

/** Daemon event carrying terminal output for one connection-local instance. */
export const TerminalOutputEvent = TerminalInstanceFrame.extend({
  type: z.literal("terminal.output"),
  data: z.string().min(1),
})

export type TerminalOutputEvent = z.infer<typeof TerminalOutputEvent>

/** Daemon event emitted when one terminal process exits. */
export const TerminalExitEvent = TerminalInstanceFrame.extend({
  type: z.literal("terminal.exit"),
  exitCode: z.number().int().nullable(),
  signal: z.string().nullable(),
})

export type TerminalExitEvent = z.infer<typeof TerminalExitEvent>

/** Daemon event emitted when a terminal title changes. */
export const TerminalTitleEvent = TerminalInstanceFrame.extend({
  type: z.literal("terminal.title"),
  title: z.string(),
})

export type TerminalTitleEvent = z.infer<typeof TerminalTitleEvent>

/** Daemon event emitted when a terminal reports a current working directory change. */
export const TerminalCwdEvent = TerminalInstanceFrame.extend({
  type: z.literal("terminal.cwd"),
  cwd: z.string().min(1),
})

export type TerminalCwdEvent = z.infer<typeof TerminalCwdEvent>

/** Stable error categories for terminal websocket failures. */
export const TerminalErrorCode = z.enum([
  "invalid-frame",
  "duplicate-instance",
  "unknown-instance",
  "spawn-failed",
  "input-failed",
  "resize-failed",
  "restart-failed",
  "close-failed",
  "internal-error",
])

export type TerminalErrorCode = z.infer<typeof TerminalErrorCode>

/** Daemon event emitted for frame, instance, or terminal runtime failures. */
export const TerminalErrorEvent = z.strictObject({
  type: z.literal("terminal.error"),
  instanceId: TerminalInstanceId.optional(),
  code: TerminalErrorCode,
  message: z.string().min(1),
  recoverable: z.boolean(),
})

export type TerminalErrorEvent = z.infer<typeof TerminalErrorEvent>

/** All terminal events emitted by one daemon terminal websocket connection. */
export const TerminalDaemonEvent = z.discriminatedUnion("type", [
  TerminalCreatedEvent,
  TerminalOutputEvent,
  TerminalExitEvent,
  TerminalTitleEvent,
  TerminalCwdEvent,
  TerminalErrorEvent,
])

export type TerminalDaemonEvent = z.infer<typeof TerminalDaemonEvent>
