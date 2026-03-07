import { runSubprocess } from "./subprocess.ts"
import type { SessionDriver, SessionDriverInput } from "./types.ts"

export function buildGeminiArgs(input: SessionDriverInput): string[] {
  const args = ["--output-format", "stream-json"]

  if (input.resume) {
    args.push("--resume", input.resume)
  }

  if (input.initialPrompt) {
    args.push("--prompt", input.initialPrompt)
  }

  return args
}

export const driver: SessionDriver = {
  name: "gemini",
  run: async (input, context) => {
    return await runSubprocess("gemini", buildGeminiArgs(input), context)
  },
}
