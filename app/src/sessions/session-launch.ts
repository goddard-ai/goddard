import type { CreateDaemonSessionRequest } from "@goddard-ai/sdk"

export function buildCreateSessionInput(draftProjectPath: string | null, draftPrompt: string) {
  const prompt = draftPrompt.trim()

  if (!draftProjectPath || prompt.length === 0) {
    return null
  }

  return {
    agent: "pi",
    cwd: draftProjectPath,
    mcpServers: [],
    systemPrompt: "",
    initialPrompt: prompt,
  } satisfies CreateDaemonSessionRequest
}
