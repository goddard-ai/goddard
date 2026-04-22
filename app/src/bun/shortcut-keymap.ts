import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getShortcutKeymapPath } from "@goddard-ai/paths/node"
import { z } from "zod"

import { UserShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"

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
    const keymap = UserShortcutKeymapFile.safeParse(parsed)

    if (!keymap.success) {
      return {
        keymap: null,
        error: `Shortcut keymap at ${keymapPath} is invalid: ${z.prettifyError(keymap.error)}`,
      } satisfies ReadShortcutKeymapResult
    }

    return {
      keymap: keymap.data,
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
      error: `Failed to read shortcut keymap at ${keymapPath}: ${getErrorMessage(error)}`,
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

/** Returns one readable message for an unknown shortcut keymap read failure. */
function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message
  }

  return typeof error === "string" && error.length > 0 ? error : "Unknown error."
}
