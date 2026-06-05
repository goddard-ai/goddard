import type {
  ListSlashCommandsRequest,
  ListSlashCommandsResponse,
  ResolvedSlashCommandDefinition,
  ResolvedSlashCommandsConfig,
  ResolveSlashCommandRequest,
  ResolveSlashCommandResponse,
  SlashCommandReferences,
} from "../schema.ts"

const DEFAULT_SLASH_COMMAND_LIMIT = 20
const MAX_SLASH_COMMAND_LIMIT = 50
const MAX_EXPANSION_DEPTH = 8

type RootConfigProvider = {
  getRootConfig: (cwd: string) => Promise<{
    config: {
      slashCommands?: ResolvedSlashCommandsConfig
    }
  }>
}

type ExpansionContext = {
  commands: ResolvedSlashCommandsConfig
  references: SlashCommandReferences
  stack: string[]
}

function normalizeLimit(limit: number | undefined) {
  return Math.min(Math.max(limit ?? DEFAULT_SLASH_COMMAND_LIMIT, 1), MAX_SLASH_COMMAND_LIMIT)
}

function formatInputHint(command: ResolvedSlashCommandDefinition) {
  if (!command.arguments || command.arguments.length === 0) {
    return null
  }

  return command.arguments
    .map((argument) => (argument.required ? `<${argument.name}>` : `[${argument.name}]`))
    .join(" ")
}

function commandMatchesQuery(
  name: string,
  command: ResolvedSlashCommandDefinition,
  normalizedQuery: string,
) {
  if (normalizedQuery.length === 0) {
    return true
  }

  return (
    name.toLowerCase().includes(normalizedQuery) ||
    (command.description?.toLowerCase().includes(normalizedQuery) ?? false) ||
    (formatInputHint(command)?.toLowerCase().includes(normalizedQuery) ?? false)
  )
}

function extractCommandName(value: string) {
  const name = value.startsWith("/") ? value.slice(1) : value
  return name.trim()
}

function addUnique(values: string[], value: string) {
  if (!values.includes(value)) {
    values.push(value)
  }
}

function trimReference(value: string) {
  return value.replace(/[.,!?;:]+$/, "")
}

function addCommandReference(
  references: SlashCommandReferences,
  name: string,
  command: ResolvedSlashCommandDefinition,
) {
  if (references.commands.some((reference) => reference.name === name)) {
    return
  }

  references.commands.push({
    name,
    source: command.source,
  })
}

function collectInlineReferences(prompt: string, references: SlashCommandReferences) {
  for (const match of prompt.matchAll(/(^|[\s([{])\$([a-zA-Z0-9][a-zA-Z0-9._-]*)/g)) {
    addUnique(references.skills, trimReference(match[2]!))
  }

  for (const match of prompt.matchAll(/(^|[\s([{])@([^\s)\]}]+)/g)) {
    addUnique(references.files, trimReference(match[2]!))
  }
}

function bindArguments(command: ResolvedSlashCommandDefinition, input: string | undefined) {
  if (!command.arguments || command.arguments.length === 0) {
    return command.prompt
  }

  const values = (input ?? "").trim().length === 0 ? [] : (input ?? "").trim().split(/\s+/)
  let prompt = command.prompt

  for (const [index, argument] of command.arguments.entries()) {
    const value = values[index] ?? argument.default

    if (value === undefined) {
      if (argument.required) {
        throw new Error(`Slash command argument "${argument.name}" is required.`)
      }
      continue
    }

    prompt = prompt.replaceAll(`{{${argument.name}}}`, value)
  }

  return prompt
}

function expandNestedCommands(prompt: string, context: ExpansionContext) {
  return prompt.replace(
    /(^|\n)\/([a-zA-Z0-9][a-zA-Z0-9._-]*)(?:[ \t]+([^\n]*))?/g,
    (_match, prefix: string, commandName: string, input: string | undefined) =>
      `${prefix}${expandCommand(commandName, input, context)}`,
  )
}

function expandCommand(name: string, input: string | undefined, context: ExpansionContext): string {
  if (context.stack.length >= MAX_EXPANSION_DEPTH) {
    throw new Error(
      `Slash command expansion exceeded ${MAX_EXPANSION_DEPTH} nested command references.`,
    )
  }

  if (context.stack.includes(name)) {
    throw new Error(
      `Slash command expansion cycle detected: ${[...context.stack, name].join(" -> ")}`,
    )
  }

  const command = context.commands[name]

  if (!command) {
    throw new Error(`Slash command "/${name}" is not defined.`)
  }

  addCommandReference(context.references, name, command)
  context.stack.push(name)

  try {
    const prompt = bindArguments(command, input)
    collectInlineReferences(prompt, context.references)
    return expandNestedCommands(prompt, context)
  } finally {
    context.stack.pop()
  }
}

export async function listSlashCommands(
  params: ListSlashCommandsRequest,
  rootConfigProvider: RootConfigProvider,
): Promise<ListSlashCommandsResponse> {
  const { config } = await rootConfigProvider.getRootConfig(params.cwd)
  const commands = config.slashCommands ?? {}
  const normalizedQuery = params.query?.trim().toLowerCase() ?? ""
  const limit = normalizeLimit(params.limit)

  return {
    commands: Object.entries(commands)
      .filter(([name, command]) => commandMatchesQuery(name, command, normalizedQuery))
      .sort(([leftName], [rightName]) => leftName.localeCompare(rightName))
      .slice(0, limit)
      .map(([name, command]) => ({
        type: "slash_command",
        name,
        description: command.description ?? "Custom slash command",
        inputHint: formatInputHint(command),
        source: command.source,
      })),
  }
}

export async function resolveSlashCommand(
  params: ResolveSlashCommandRequest,
  rootConfigProvider: RootConfigProvider,
): Promise<ResolveSlashCommandResponse> {
  const name = extractCommandName(params.name)
  const { config } = await rootConfigProvider.getRootConfig(params.cwd)
  const commands = config.slashCommands ?? {}
  const command = commands[name]

  if (!command) {
    throw new Error(`Slash command "/${name}" is not defined.`)
  }

  const references: SlashCommandReferences = {
    commands: [],
    files: [],
    skills: [],
  }
  const prompt = expandCommand(name, params.input, {
    commands,
    references,
    stack: [],
  })

  return {
    name,
    source: command.source,
    prompt,
    session: command.session,
    references,
  }
}
