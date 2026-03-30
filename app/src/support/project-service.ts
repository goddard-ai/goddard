import { browseForProject, inspectProjectPath } from "../desktop-host"

/**
 * Opens one native directory picker for selecting a project root.
 */
export async function pickProjectPath(): Promise<string | null> {
  return await browseForProject()
}

/**
 * Validates one project path through the desktop host bridge.
 */
export async function validateProjectPath(path: string) {
  return await inspectProjectPath(path)
}
