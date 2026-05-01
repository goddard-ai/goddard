import { z } from "zod"

export const terminalIdSchema = z.string().min(1)

export type TerminalId = z.infer<typeof terminalIdSchema>
