import path from "node:path"

export const sprintStateFileName = ".sprint-branch-state.json"
export const sprintIndexFileName = "000-index.md"
export const sprintHandoffFileName = "001-handoff.md"

/** Returns the canonical state path for the inferred sprint context. */
export function sprintStatePath(rootDir: string, sprint: string) {
  return path.join(rootDir, "sprints", sprint, sprintStateFileName)
}

/** Returns the canonical index mirror path for the inferred sprint context. */
export function sprintIndexPath(rootDir: string, sprint: string) {
  return path.join(rootDir, "sprints", sprint, sprintIndexFileName)
}

/** Returns the canonical handoff log path for the inferred sprint context. */
export function sprintHandoffPath(rootDir: string, sprint: string) {
  return path.join(rootDir, "sprints", sprint, sprintHandoffFileName)
}
