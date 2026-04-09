import { getShortcutKeymapPath } from "@goddard-ai/paths/node"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { parseShortcutKeymapFile, type UserShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"

/** Result of reading the persisted user shortcut keymap file. */
export type ReadShortcutKeymapResult = {
  keymap: UserShortcutKeymapFile | null
  error: string | null
}

/** Reads the user-scoped shortcut keymap file when it exists and is valid JSON. */
export async function readShortcutKeymap() {
  const keymapPath = getShortcutKeymapPath()

  try {
    const parsed = JSON.parse(await readFile(keymapPath, "utf-8")) as unknown
    const keymap = parseShortcutKeymapFile(parsed)

    if (!keymap) {
      return {
        keymap: null,
        error: `Shortcut keymap at ${keymapPath} is invalid.`,
      } satisfies ReadShortcutKeymapResult
    }

    return {
      keymap,
      error: null,
    } satisfies ReadShortcutKeymapResult
  } catch (error) {
    if (isFileMissingError(error)) {
      return {
        keymap: null,
        error: null,
      } satisfies ReadShortcutKeymapResult
    }

    return {
      keymap: null,
      error: `Failed to read shortcut keymap at ${keymapPath}.`,
    } satisfies ReadShortcutKeymapResult
  }
}

/** Writes the user-scoped shortcut keymap file, creating its parent directory when needed. */
export async function writeShortcutKeymap(keymap: UserShortcutKeymapFile) {
  const keymapPath = getShortcutKeymapPath()

  await mkdir(dirname(keymapPath), { recursive: true })
  await writeFile(keymapPath, `${JSON.stringify(keymap, null, 2)}\n`, "utf-8")

  return keymap
}

/** Returns whether one unknown read failure means the keymap file does not exist yet. */
function isFileMissingError(error: unknown) {
  return Boolean(error && typeof error === "object" && "code" in error && error.code === "ENOENT")
}
