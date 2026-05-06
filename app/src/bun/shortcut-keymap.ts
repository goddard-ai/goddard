import { getShortcutKeymapPath } from "@goddard-ai/paths/node"

import { ShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"
import { readJsonFile, writeJsonFile } from "./json-file.ts"

/** Reads the app-only shortcut keymap JSON file from the Goddard user directory. */
export async function loadShortcutKeymap() {
  return readJsonFile(getShortcutKeymapPath(), ShortcutKeymapFile)
}

/** Atomically writes the app-only shortcut keymap JSON file in the Goddard user directory. */
export async function writeShortcutKeymap(keymap: ShortcutKeymapFile) {
  await writeJsonFile(getShortcutKeymapPath(), keymap)
}
