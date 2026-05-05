import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { getShortcutKeymapPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import type { ShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"
import { loadShortcutKeymap, writeShortcutKeymap } from "./shortcut-keymap.ts"

const originalHome = process.env.HOME
const tempHomes: string[] = []

afterEach(async () => {
  restoreEnv("HOME", originalHome)

  await Promise.all(tempHomes.splice(0).map((home) => rm(home, { force: true, recursive: true })))
})

test("shortcut keymap persistence returns null when the keymap file is absent", async () => {
  process.env.HOME = await createTempHome()

  await expect(loadShortcutKeymap()).resolves.toBeNull()
})

test("shortcut keymap persistence writes and reads the Goddard user keymap file", async () => {
  process.env.HOME = await createTempHome()
  const keymap = {
    version: 1,
    selectedProfileId: "goddard",
    overrides: {
      "navigation.openKeyboardShortcuts": ["Mod+/"],
      "navigation.openInbox": null,
    },
  } satisfies ShortcutKeymapFile

  await writeShortcutKeymap(keymap)

  await expect(loadShortcutKeymap()).resolves.toEqual(keymap)
  await expect(readFile(getShortcutKeymapPath(), "utf8")).resolves.toBe(
    `${JSON.stringify(keymap, null, 2)}\n`,
  )
})

test("shortcut keymap persistence rejects invalid keymap files", async () => {
  process.env.HOME = await createTempHome()
  await mkdir(dirname(getShortcutKeymapPath()), { recursive: true })
  await writeFile(
    getShortcutKeymapPath(),
    JSON.stringify({
      version: 1,
      selectedProfileId: "goddard",
      overrides: {
        "navigation.openKeyboardShortcuts": [],
      },
    }),
    "utf8",
  )

  await expect(loadShortcutKeymap()).rejects.toThrow()
})

async function createTempHome() {
  const home = await mkdtemp(join(tmpdir(), "goddard-shortcut-keymap-"))
  tempHomes.push(home)
  return home
}

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
