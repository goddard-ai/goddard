import type { SprintDiagnostic } from "./types"

/** Reports whether any diagnostic is severe enough to make a command fail. */
export function hasDiagnosticErrors(diagnostics: SprintDiagnostic[]) {
  return diagnostics.some((diagnostic) => diagnostic.severity === "error")
}
