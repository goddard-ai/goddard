import type { GoddardSdk, SessionId } from "@goddard-ai/sdk"

import type { SessionInputSuggestion, SessionInputTrigger } from "~/session-input/input.tsrx"

type ComposerSuggestionSdk = {
  fileSearch: Pick<GoddardSdk["fileSearch"], "composerEntries">
  session: Pick<GoddardSdk["session"], "composerSuggestions">
}

export async function loadSessionChatComposerSuggestions(input: {
  cwd: string
  query: string
  sdk: ComposerSuggestionSdk
  sessionId: SessionId
  trigger: SessionInputTrigger
}): Promise<readonly SessionInputSuggestion[]> {
  if (input.trigger === "at") {
    const response = await input.sdk.fileSearch.composerEntries({
      cwd: input.cwd,
      query: input.query,
    })

    return response.entries
  }

  const response = await input.sdk.session.composerSuggestions({
    id: input.sessionId,
    trigger: input.trigger,
    query: input.query,
  })

  return response.suggestions
}
