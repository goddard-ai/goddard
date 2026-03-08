import { z } from "zod"

export const sessionPayloadRoleSchema = z.enum(["assistant", "user", "system", "tool"])
export const sessionPayloadDriverSchema = z.enum(["pi", "pi-rpc", "gemini", "codex", "pty", "unknown"])
export const sessionPayloadFormatSchema = z.enum(["json-line", "terminal"])

export const sessionPayloadSourceSchema = z.object({
  driver: sessionPayloadDriverSchema,
  format: sessionPayloadFormatSchema,
})

export const sessionPayloadBaseSchema = z.object({
  schemaVersion: z.literal(1),
  source: sessionPayloadSourceSchema,
  id: z.string().optional(),
  done: z.boolean().optional(),
  raw: z.unknown(),
})

export const sessionPayloadUsageSchema = z.object({
  inputTokens: z.number().optional(),
  outputTokens: z.number().optional(),
  totalTokens: z.number().optional(),
})

export const sessionPayloadToolCallSchema = z.object({
  name: z.string().optional(),
  arguments: z.unknown().optional(),
})

export const sessionPayloadToolResultSchema = z.object({
  name: z.string().optional(),
  result: z.unknown().optional(),
})

export const sessionPayloadCursorSchema = z.object({
  x: z.number(),
  y: z.number(),
})

export const sessionPayloadTerminalStateSchema = z.object({
  cols: z.number(),
  rows: z.number(),
  lines: z.array(z.string()),
  cursor: sessionPayloadCursorSchema,
})

export const deltaPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("delta"),
  role: sessionPayloadRoleSchema.optional(),
  text: z.string().optional(),
})

export const messagePayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("message"),
  role: sessionPayloadRoleSchema.optional(),
  text: z.string().optional(),
  message: z.string().optional(),
})

export const toolCallPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("tool_call"),
  tool: sessionPayloadToolCallSchema.optional(),
})

export const toolResultPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("tool_result"),
  tool: sessionPayloadToolResultSchema.optional(),
})

export const usagePayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("usage"),
  usage: sessionPayloadUsageSchema.optional(),
})

export const errorPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("error"),
  message: z.string().optional(),
})

export const statusPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("status"),
  message: z.string().optional(),
})

export const terminalPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("terminal"),
  terminal: sessionPayloadTerminalStateSchema,
})

export const unknownPayloadSchema = sessionPayloadBaseSchema.extend({
  kind: z.literal("unknown"),
})

export const normalizedSessionPayloadSchema = z.discriminatedUnion("kind", [
  deltaPayloadSchema,
  messagePayloadSchema,
  toolCallPayloadSchema,
  toolResultPayloadSchema,
  usagePayloadSchema,
  errorPayloadSchema,
  statusPayloadSchema,
  terminalPayloadSchema,
  unknownPayloadSchema,
])

export type SessionPayloadRole = z.infer<typeof sessionPayloadRoleSchema>
export type SessionPayloadDriver = z.infer<typeof sessionPayloadDriverSchema>
export type SessionPayloadFormat = z.infer<typeof sessionPayloadFormatSchema>
export type SessionPayloadSource = z.infer<typeof sessionPayloadSourceSchema>
export type SessionPayloadBase = z.infer<typeof sessionPayloadBaseSchema>
export type SessionPayloadUsage = z.infer<typeof sessionPayloadUsageSchema>
export type SessionPayloadToolCall = z.infer<typeof sessionPayloadToolCallSchema>
export type SessionPayloadToolResult = z.infer<typeof sessionPayloadToolResultSchema>
export type SessionPayloadCursor = z.infer<typeof sessionPayloadCursorSchema>
export type SessionPayloadTerminalState = z.infer<typeof sessionPayloadTerminalStateSchema>
export type DeltaPayload = z.infer<typeof deltaPayloadSchema>
export type MessagePayload = z.infer<typeof messagePayloadSchema>
export type ToolCallPayload = z.infer<typeof toolCallPayloadSchema>
export type ToolResultPayload = z.infer<typeof toolResultPayloadSchema>
export type UsagePayload = z.infer<typeof usagePayloadSchema>
export type ErrorPayload = z.infer<typeof errorPayloadSchema>
export type StatusPayload = z.infer<typeof statusPayloadSchema>
export type TerminalPayload = z.infer<typeof terminalPayloadSchema>
export type UnknownPayload = z.infer<typeof unknownPayloadSchema>
export type NormalizedSessionPayload = z.infer<typeof normalizedSessionPayloadSchema>
