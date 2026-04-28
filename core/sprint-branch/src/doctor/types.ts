import type { SprintConflictState } from "../types"

/** Extra recovery context that doctor needs beyond canonical sprint JSON. */
export type DoctorContext = {
  transientConflict: SprintConflictState | null
  gitOperations: Array<{ name: string; path: string }>
}
