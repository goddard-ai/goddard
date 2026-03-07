import { runSubprocess } from "./subprocess.ts"
import type { SessionPlugin, SessionPluginInput } from "./types.ts"

export function buildGeminiArgs(input: SessionPluginInput): string[] {
  const args = ["--output-format", "stream-json"]

  if (input.resume) {
    args.push("--resume", input.resume)
  }

  if (input.initialPrompt) {
    args.push("--prompt", input.initialPrompt)
  }

  return args
}

export const plugin: SessionPlugin = {
  name: "gemini",
  run: async (input, context) => {
    return await runSubprocess("gemini", buildGeminiArgs(input), context)
  },
}
