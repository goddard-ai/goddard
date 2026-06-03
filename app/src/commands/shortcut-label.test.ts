import { expect, test } from "bun:test"

import { getPreferredShortcutLabel } from "./shortcut-label.ts"

test("getPreferredShortcutLabel returns the preferred binding when it is still configured", () => {
  expect(getPreferredShortcutLabel(["Mod+Shift+m", "Mod+/"], "Mod+/")).toBe("Mod+/")
})

test("getPreferredShortcutLabel falls back to the first configured binding", () => {
  expect(getPreferredShortcutLabel(["Mod+Shift+m"], "Mod+/")).toBe("Mod+Shift+m")
})
