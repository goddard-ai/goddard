import { expect, test } from "bun:test"
import { wcagContrast } from "culori/fn"
import { buildAppearanceDocumentState } from "./theme.ts"

function contrastBetween(
  documentState: ReturnType<typeof buildAppearanceDocumentState>,
  firstVariableName: keyof ReturnType<typeof buildAppearanceDocumentState>["variables"],
  secondVariableName: keyof ReturnType<typeof buildAppearanceDocumentState>["variables"],
) {
  return wcagContrast(
    documentState.variables[firstVariableName],
    documentState.variables[secondVariableName],
  )
}

test("buildAppearanceDocumentState resolves system mode to the observed dark theme", () => {
  const documentState = buildAppearanceDocumentState({
    mode: "system",
    highContrast: false,
    systemTheme: "dark",
  })

  expect(documentState.themeName).toBe("dark")
  expect(documentState.attributes["data-theme"]).toBe("dark")
  expect(documentState.attributes["data-theme-mode"]).toBe("system")
})

test("buildAppearanceDocumentState keeps default text readable against the canvas", () => {
  const documentState = buildAppearanceDocumentState({
    mode: "light",
    highContrast: false,
    systemTheme: "light",
  })

  expect(
    contrastBetween(documentState, "--theme-color-background", "--theme-color-text"),
  ).toBeGreaterThan(11)
  expect(
    contrastBetween(documentState, "--theme-color-background", "--theme-color-accent"),
  ).toBeGreaterThan(2.7)
})

test("buildAppearanceDocumentState strengthens muted and border contrast in high-contrast mode", () => {
  const normalContrast = buildAppearanceDocumentState({
    mode: "dark",
    highContrast: false,
    systemTheme: "dark",
  })
  const highContrast = buildAppearanceDocumentState({
    mode: "dark",
    highContrast: true,
    systemTheme: "dark",
  })

  expect(
    contrastBetween(highContrast, "--theme-color-background", "--theme-color-muted"),
  ).toBeGreaterThan(
    contrastBetween(normalContrast, "--theme-color-background", "--theme-color-muted"),
  )
  expect(
    contrastBetween(highContrast, "--theme-color-background", "--theme-color-border"),
  ).toBeGreaterThan(
    contrastBetween(normalContrast, "--theme-color-background", "--theme-color-border"),
  )
})
