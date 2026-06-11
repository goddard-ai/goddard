import type { GoddardSdk, SessionLaunchPreviewResponse } from "@goddard-ai/sdk"

import type { SessionInputSuggestion, SessionInputTrigger } from "~/session-input/input.tsrx"
import { filterSlashCommandSuggestions } from "./launch-form-state.ts"

type LaunchSuggestionSdk = {
  fileSearch: Pick<GoddardSdk["fileSearch"], "composerEntries">
  session: Pick<GoddardSdk["session"], "draftSuggestions">
}

export async function loadSessionLaunchComposerSuggestions(input: {
  cwd: string
  query: string
  sdk: LaunchSuggestionSdk
  slashCommands: SessionLaunchPreviewResponse["slashCommands"]
  trigger: SessionInputTrigger
}): Promise<readonly SessionInputSuggestion[]> {
  if (input.trigger === "slash") {
    return filterSlashCommandSuggestions(input.slashCommands, input.query)
  }

  if (input.trigger === "at") {
    const response = await input.sdk.fileSearch.composerEntries({
      cwd: input.cwd,
      query: input.query,
    })

    return response.entries
  }

  const response = await input.sdk.session.draftSuggestions({
    cwd: input.cwd,
    trigger: input.trigger,
    query: input.query,
  })

  return response.suggestions
}
