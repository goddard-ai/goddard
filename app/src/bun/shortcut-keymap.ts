import { randomUUID } from "node:crypto"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getShortcutKeymapPath } from "@goddard-ai/paths/node"

import { ShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"

function isNodeErrorCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code
}

/** Reads the app-only shortcut keymap JSON file from the Goddard user directory. */
export async function loadShortcutKeymap() {
  let source: string

  try {
    source = await readFile(getShortcutKeymapPath(), "utf8")
  } catch (error) {
    if (isNodeErrorCode(error, "ENOENT")) {
      return null
    }

    throw error
  }

  return ShortcutKeymapFile.parse(JSON.parse(source))
}

/** Atomically writes the app-only shortcut keymap JSON file in the Goddard user directory. */
export async function writeShortcutKeymap(keymap: ShortcutKeymapFile) {
  const keymapPath = getShortcutKeymapPath()
  const temporaryPath = `${keymapPath}.${process.pid}.${randomUUID()}.tmp`

  await mkdir(dirname(keymapPath), { recursive: true })

  try {
    await writeFile(temporaryPath, `${JSON.stringify(keymap, null, 2)}\n`, "utf8")
    await rename(temporaryPath, keymapPath)
  } catch (error) {
    await rm(temporaryPath, { force: true }).catch(() => {})
    throw error
  }
}
