/** Shared helpers for deriving stable project metadata from local folder paths. */

/** Derives a default project name from the last path segment when present. */
export function deriveProjectName(path: string): string {
  const trimmedPath = path.trim().replace(/[\\/]+$/, "")

  if (trimmedPath.length === 0) {
    return ""
  }

  const segments = trimmedPath.split(/[\\/]/)
  return segments.at(-1) ?? ""
}
