import { StaticSessionParams } from "@goddard-ai/schema/config"
import { z } from "zod"

/** Stable custom slash command name without the leading slash. */
export const SlashCommandName = z
  .string()
  .regex(
    /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/,
    "Slash command names must contain only letters, numbers, dots, underscores, or hyphens.",
  )

export type SlashCommandName = z.infer<typeof SlashCommandName>

/** One named argument accepted by a custom slash command. */
export const SlashCommandArgument = z.strictObject({
  name: z.string().min(1),
  description: z.string().min(1).optional(),
  required: z.boolean().optional(),
  default: z.string().optional(),
})

export type SlashCommandArgument = z.infer<typeof SlashCommandArgument>

/** Persisted custom slash command definition loaded from root Goddard config. */
export const SlashCommandDefinition = z.strictObject({
  description: z.string().min(1).optional(),
  prompt: z.string().min(1),
  arguments: z.array(SlashCommandArgument).optional(),
  session: StaticSessionParams.optional(),
})

export type SlashCommandDefinition = z.infer<typeof SlashCommandDefinition>

/** User or project root config section containing custom slash commands. */
export const SlashCommandsConfig = z
  .record(SlashCommandName, SlashCommandDefinition)
  .describe("Custom slash commands keyed by command name without the leading slash.")

export type SlashCommandsConfig = z.infer<typeof SlashCommandsConfig>

export type SlashCommandSource = "user" | "project"

/** Resolved custom slash command with the config layer that supplied it. */
export type ResolvedSlashCommandDefinition = SlashCommandDefinition & {
  source: SlashCommandSource
}

export type ResolvedSlashCommandsConfig = Record<string, ResolvedSlashCommandDefinition>

/** Merges user and project slash commands while preserving command provenance. */
export function mergeSlashCommandsConfigLayers(input: {
  user?: SlashCommandsConfig
  project?: SlashCommandsConfig
}): ResolvedSlashCommandsConfig | undefined {
  if (!input.user && !input.project) {
    return undefined
  }

  const commands: ResolvedSlashCommandsConfig = {}

  for (const [name, command] of Object.entries(input.user ?? {})) {
    commands[name] = {
      ...command,
      source: "user",
    }
  }

  for (const [name, command] of Object.entries(input.project ?? {})) {
    commands[name] = {
      ...command,
      source: "project",
    }
  }

  return commands
}

export const SlashCommandReference = z.strictObject({
  name: z.string(),
  source: z.enum(["user", "project"]),
})

export type SlashCommandReference = z.infer<typeof SlashCommandReference>

export const SlashCommandReferences = z.strictObject({
  commands: z.array(SlashCommandReference),
  files: z.array(z.string()),
  skills: z.array(z.string()),
})

export type SlashCommandReferences = z.infer<typeof SlashCommandReferences>

export const ListSlashCommandsRequest = z.strictObject({
  cwd: z.string().min(1),
  query: z.string().optional(),
  limit: z.number().int().positive().optional(),
})

export type ListSlashCommandsRequest = z.infer<typeof ListSlashCommandsRequest>

export const SlashCommandSummary = z.strictObject({
  type: z.literal("slash_command"),
  name: z.string(),
  description: z.string(),
  inputHint: z.string().nullable(),
  source: z.enum(["user", "project"]),
})

export type SlashCommandSummary = z.infer<typeof SlashCommandSummary>

export type ListSlashCommandsResponse = {
  commands: SlashCommandSummary[]
}

export const ResolveSlashCommandRequest = z.strictObject({
  cwd: z.string().min(1),
  name: z.string().min(1),
  input: z.string().optional(),
})

export type ResolveSlashCommandRequest = z.infer<typeof ResolveSlashCommandRequest>

export type ResolveSlashCommandResponse = {
  name: string
  source: SlashCommandSource
  prompt: string
  session?: SlashCommandDefinition["session"]
  references: SlashCommandReferences
}
