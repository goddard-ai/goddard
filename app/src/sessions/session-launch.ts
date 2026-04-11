import type { CreateSessionRequest } from "@goddard-ai/sdk"

export function buildCreateSessionInput(
  draftProjectPath: string | null,
  draftAdapterId: string | null,
  draftPrompt: string,
) {
  const prompt = draftPrompt.trim()

  if (!draftProjectPath || !draftAdapterId || prompt.length === 0) {
    return null
  }

  return {
    agent: draftAdapterId,
    cwd: draftProjectPath,
    mcpServers: [],
    systemPrompt: "",
    initialPrompt: prompt,
  } satisfies CreateSessionRequest
}
