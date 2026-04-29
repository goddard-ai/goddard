import * as fs from "node:fs/promises"
import path from "node:path"

import { sprintStateDisplayPath, sprintStatePath } from "../state/paths"
import { clearTransientConflict } from "../transient-conflict"
import type { SprintBranchState } from "../types"

/** Writes canonical sprint branch state outside the repository working tree. */
export async function writeSprintState(rootDir: string, state: SprintBranchState) {
  await writeSprintStateAtomic(await sprintStatePath(rootDir, state.sprint), {
    ...state,
    lock: null,
  })
  await clearTransientConflict(rootDir, state.sprint)
}

/** Lists state files that a mutation plans to update. */
export function stateFilesForState(state: SprintBranchState) {
  return [sprintStateDisplayPath(state.sprint)]
}

async function writeSprintStateAtomic(statePath: string, state: SprintBranchState) {
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  const tempPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await fs.writeFile(tempPath, `${JSON.stringify(state, null, 2)}\n`)
  await fs.rename(tempPath, statePath)
}
