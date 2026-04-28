import { confirm, isCancel } from "@clack/prompts"

import type { SprintDiagnostic } from "../types"
import type { HumanCommandInput } from "./types"

/** Requires an interactive human confirmation before landing or cleanup mutates Git. */
export async function confirmHumanAction(
  input: HumanCommandInput,
  diagnostics: SprintDiagnostic[],
  message: string,
) {
  if (!process.stdin.isTTY || !process.stdout.isTTY || input.json) {
    return false
  }

  const confirmed = await confirm({ message, initialValue: false })
  if (isCancel(confirmed) || !confirmed) {
    diagnostics.push({
      severity: "error",
      code: "human_confirmation_required",
      message: "Command cancelled before mutating branches or worktrees.",
    })
    return false
  }

  return true
}
