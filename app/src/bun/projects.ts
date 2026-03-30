import { statSync } from "node:fs"
import { basename } from "node:path"
import { Utils } from "electrobun/bun"
import type { ProjectInspection } from "../shared/desktop-rpc"

/**
 * Opens one native directory picker for selecting a project root.
 */
export async function browseForProject(): Promise<string | null> {
  const [selectedPath] = await Utils.openFileDialog({
    canChooseFiles: false,
    canChooseDirectory: true,
    allowsMultipleSelection: false,
  })

  return selectedPath ?? null
}

/**
 * Validates one local path and derives the minimal project metadata used by the app shell.
 */
export async function inspectProject(path: string): Promise<ProjectInspection> {
  if (path.length === 0) {
    throw new Error("Enter a project path.")
  }

  if (!isExistingDirectory(path)) {
    throw new Error("The selected path is not an existing directory.")
  }

  return {
    path,
    name: basename(path),
  }
}

/**
 * Returns whether the candidate path exists and is a directory.
 */
function isExistingDirectory(path: string): boolean {
  try {
    return statSync(path).isDirectory()
  } catch {
    return false
  }
}
