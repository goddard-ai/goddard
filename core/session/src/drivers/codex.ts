import { runSubprocess } from "./subprocess.ts"
import type { SessionDriver, SessionDriverInput } from "./types.ts"

export function buildCodexArgs(input: SessionDriverInput): string[] {
  const args = ["exec"]

  if (input.resume) {
    args.push("resume", input.resume)
  }

  args.push("--json")

  if (input.initialPrompt) {
    args.push(input.initialPrompt)
  }

  return args
}

export const driver: SessionDriver = {
  name: "codex",
  run: async (input, context) => {
    return await runSubprocess("codex", buildCodexArgs(input), context)
  },
}
