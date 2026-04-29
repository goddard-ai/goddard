import path from "node:path"

import { resolveGitCommonPath } from "../git/repository"

export const sprintStateFileName = "state.json"
export const sprintStateRoot = "sprint-branch"

/** Returns the logical Git metadata path for the canonical sprint state file. */
export function sprintStateGitPath(sprint: string) {
  return path.join(sprintStateRoot, sprint, sprintStateFileName)
}

/** Returns the canonical state path in Git metadata shared by linked worktrees. */
export async function sprintStatePath(rootDir: string, sprint: string) {
  return resolveGitCommonPath(rootDir, sprintStateGitPath(sprint))
}

/** Returns a stable display path for state stored outside the working tree. */
export function sprintStateDisplayPath(sprint: string) {
  return path.join(".git", sprintStateGitPath(sprint))
}
