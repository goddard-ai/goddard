import { z } from "zod"

import { normalizedSessionPayloadSchema, sessionPayloadTerminalStateSchema } from "./payloads.ts"

export const sessionClientTextEventSchema = z.object({
  type: z.literal("input.text"),
  text: z.string().min(1),
})

export const sessionClientTerminalEventSchema = z.object({
  type: z.literal("input.terminal"),
  data: z.string().min(1),
})

export const sessionTerminalResizeEventSchema = z.object({
  type: z.literal("terminal.resize"),
  cols: z.number(),
  rows: z.number(),
})

export const sessionClientEventSchema = z.discriminatedUnion("type", [
  sessionClientTextEventSchema,
  sessionClientTerminalEventSchema,
  sessionTerminalResizeEventSchema,
])

export const sessionStartupInputSchema = z.object({
  resume: z.string().optional(),
})

export const sessionInitializeParamsSchema = z
  .object({
    input: sessionStartupInputSchema.optional(),
  })
  .optional()

export const sessionTextOutputEventSchema = z.object({
  type: z.literal("output.text"),
  text: z.string(),
})

export const sessionTerminalOutputEventSchema = z.object({
  type: z.literal("output.terminal"),
  data: z.string(),
})

export const sessionNormalizedOutputEventSchema = z.object({
  type: z.literal("output.normalized"),
  payload: normalizedSessionPayloadSchema,
})

export const sessionExitEventSchema = z.object({
  type: z.literal("session.exit"),
  exitCode: z.number(),
})

export const sessionErrorEventSchema = z.object({
  type: z.literal("session.error"),
  message: z.string(),
})

export const sessionServerEventSchema = z.discriminatedUnion("type", [
  sessionTextOutputEventSchema,
  sessionTerminalOutputEventSchema,
  sessionNormalizedOutputEventSchema,
  sessionExitEventSchema,
  sessionErrorEventSchema,
])

export const sessionDriverCapabilitiesSchema = z.object({
  terminal: z.object({
    enabled: z.boolean(),
    canResize: z.boolean(),
    hasScreenState: z.boolean(),
  }),
  normalizedOutput: z.boolean(),
})

export const sessionTerminalStateSchema = sessionPayloadTerminalStateSchema

export const sessionInitializeResultSchema = z.object({
  protocolVersion: z.literal(1),
  driver: z.string(),
  capabilities: sessionDriverCapabilitiesSchema,
  state: z.object({
    terminal: sessionTerminalStateSchema.nullable(),
  }),
})

export const sessionGetStateResultSchema = z.object({
  terminal: sessionTerminalStateSchema.nullable(),
})

export const sessionEventNotificationSchema = z.object({
  method: z.literal("session_event"),
  params: z.object({
    sequence: z.number(),
    event: sessionServerEventSchema,
  }),
})

export type SessionClientEvent = z.infer<typeof sessionClientEventSchema>
export type SessionStartupInput = z.infer<typeof sessionStartupInputSchema>
export type SessionInitializeParams = z.infer<typeof sessionInitializeParamsSchema>
export type SessionServerEvent = z.infer<typeof sessionServerEventSchema>
export type SessionDriverCapabilities = z.infer<typeof sessionDriverCapabilitiesSchema>
export type SessionTerminalState = z.infer<typeof sessionTerminalStateSchema>
export type SessionInitializeResult = z.infer<typeof sessionInitializeResultSchema>
export type SessionGetStateResult = z.infer<typeof sessionGetStateResultSchema>
export type SessionEventNotification = z.infer<typeof sessionEventNotificationSchema>
