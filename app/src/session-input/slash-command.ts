import type { SessionComposerSuggestionsResponse, SessionPromptRequest } from "@goddard-ai/sdk"

import { goddardSdk } from "~/sdk.ts"

type PromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
type SlashCommandSuggestion = SessionComposerSuggestionsResponse["suggestions"][number]

function readLeadingSlashCommand(blocks: PromptBlocks) {
  const firstBlock = blocks[0]

  if (firstBlock?.type !== "text") {
    return null
  }

  const match = /^\s*\/([a-zA-Z0-9][a-zA-Z0-9._-]*)(?:[ \t]+([^\n]*))?(\n[\s\S]*)?$/.exec(
    firstBlock.text,
  )

  if (!match) {
    return null
  }

  return {
    name: match[1]!,
    input: match[2],
    suffix: match[3]?.slice(1) ?? "",
  }
}

export async function listCustomSlashCommandSuggestions(input: {
  cwd: string
  query: string
}): Promise<SlashCommandSuggestion[]> {
  const response = await goddardSdk.slashCommand.list(input)
  return response.commands
}

export async function resolveLeadingCustomSlashCommand(input: {
  cwd: string
  prompt: PromptBlocks
}): Promise<PromptBlocks> {
  const command = readLeadingSlashCommand(input.prompt)

  if (!command) {
    return input.prompt
  }

  let resolved: Awaited<ReturnType<typeof goddardSdk.slashCommand.resolve>>

  try {
    resolved = await goddardSdk.slashCommand.resolve({
      cwd: input.cwd,
      name: command.name,
      input: command.input,
    })
  } catch (error) {
    if (
      error instanceof Error &&
      error.message === `Slash command "/${command.name}" is not defined.`
    ) {
      return input.prompt
    }

    throw error
  }
  const resolvedText =
    command.suffix.trim().length === 0 ? resolved.prompt : `${resolved.prompt}\n${command.suffix}`

  return [
    {
      type: "text",
      text: resolvedText,
    },
    ...input.prompt.slice(1),
  ]
}
