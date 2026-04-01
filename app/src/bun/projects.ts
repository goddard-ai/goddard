import { Utils } from "electrobun/bun"

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
