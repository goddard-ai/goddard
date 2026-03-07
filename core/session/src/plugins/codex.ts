import { runSubprocess } from "./subprocess.ts"
import type { SessionPlugin, SessionPluginInput } from "./types.ts"

export function buildCodexArgs(input: SessionPluginInput): string[] {
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

export const plugin: SessionPlugin = {
  name: "codex",
  run: async (input, context) => {
    return await runSubprocess("codex", buildCodexArgs(input), context)
  },
}
