import { readFileSync } from "node:fs"
import { expect, test } from "vitest"

import {
  BOOT_APPEARANCE_STORAGE_KEY,
  parseBootAppearanceSnapshot,
  readBootAppearanceSnapshot,
  writeBootAppearanceSnapshot,
} from "./boot-appearance.ts"

function resetDocumentTheme() {
  const root = document.documentElement

  root.removeAttribute("data-theme")
  root.removeAttribute("data-theme-mode")
  root.removeAttribute("data-contrast")
  root.removeAttribute("style")
}

function runInlineBootScript() {
  const html = readFileSync("src/main/index.html", "utf8")
  const script = html.match(/<script>\s*([\S\s]*?)\s*<\/script>/)?.[1]

  expect(typeof script).toBe("string")
  Function(script ?? "")()
}

test("parseBootAppearanceSnapshot accepts the minimal first-paint appearance hint", () => {
  expect(parseBootAppearanceSnapshot(JSON.stringify({ mode: "dark", highContrast: true }))).toEqual(
    {
      mode: "dark",
      highContrast: true,
    },
  )
})

test("parseBootAppearanceSnapshot rejects invalid or incomplete cached appearance", () => {
  expect(parseBootAppearanceSnapshot(null)).toBeNull()
  expect(parseBootAppearanceSnapshot("{")).toBeNull()
  expect(
    parseBootAppearanceSnapshot(JSON.stringify({ mode: "sepia", highContrast: false })),
  ).toBeNull()
  expect(parseBootAppearanceSnapshot(JSON.stringify({ mode: "dark" }))).toBeNull()
})

test("readBootAppearanceSnapshot falls back to system mode when cache is missing", () => {
  expect(readBootAppearanceSnapshot(localStorage)).toEqual({
    mode: "system",
    highContrast: false,
  })
})

test("writeBootAppearanceSnapshot stores the next first-paint appearance hint", () => {
  writeBootAppearanceSnapshot({ mode: "light", highContrast: true }, localStorage)

  expect(localStorage.getItem(BOOT_APPEARANCE_STORAGE_KEY)).toBe(
    JSON.stringify({ mode: "light", highContrast: true }),
  )
  expect(readBootAppearanceSnapshot(localStorage)).toEqual({
    mode: "light",
    highContrast: true,
  })
})

test("inline boot script applies cached dark appearance before app modules load", () => {
  resetDocumentTheme()
  localStorage.setItem(
    BOOT_APPEARANCE_STORAGE_KEY,
    JSON.stringify({ mode: "dark", highContrast: false }),
  )

  runInlineBootScript()

  expect(document.documentElement.getAttribute("data-theme")).toBe("dark")
  expect(document.documentElement.getAttribute("data-theme-mode")).toBe("dark")
  expect(document.documentElement.getAttribute("data-contrast")).toBe("normal")
  expect(document.documentElement.style.colorScheme).toBe("dark")
  expect(document.documentElement.style.getPropertyValue("--theme-color-background")).toBe(
    "rgb(19, 22, 26)",
  )
  expect(document.documentElement.style.backgroundColor).toBe("rgb(19, 22, 26)")
})

test("inline boot script ignores invalid cache and falls back to system appearance", () => {
  resetDocumentTheme()
  localStorage.setItem(BOOT_APPEARANCE_STORAGE_KEY, JSON.stringify({ mode: "sepia" }))

  runInlineBootScript()

  expect(document.documentElement.getAttribute("data-theme")).toBe("light")
  expect(document.documentElement.getAttribute("data-theme-mode")).toBe("system")
  expect(document.documentElement.style.getPropertyValue("--theme-color-background")).toBe(
    "rgb(247, 250, 254)",
  )
})
