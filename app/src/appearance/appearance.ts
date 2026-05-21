import { effect, signal } from "@preact/signals"
import { listen, Sigma } from "preact-sigma"

import {
  buildAppearanceDocumentState,
  type AppearanceMode,
  type BuiltInThemeName,
} from "./theme.ts"

/** Public state for the app shell's appearance model. */
export type AppearanceState = {
  mode: AppearanceMode
  highContrast: boolean
}

export class Appearance extends Sigma<AppearanceState> {
  /** Tracks the browser's color-scheme outside persisted state because it is runtime-derived. */
  #systemTheme = signal<BuiltInThemeName>("dark")

  get effectiveTheme() {
    return this.mode === "system" ? this.#systemTheme.value : this.mode
  }

  setMode(mode: AppearanceMode) {
    this.mode = mode
  }

  setHighContrast(highContrast: boolean) {
    this.highContrast = highContrast
  }

  onSetup() {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)")
    const syncSystemTheme = () => {
      this.#systemTheme.value = mediaQuery.matches ? "dark" : "light"
    }

    syncSystemTheme()

    return [
      listen(mediaQuery, "change", syncSystemTheme),
      effect(() => {
        const root = document.documentElement
        const documentState = buildAppearanceDocumentState({
          mode: this.mode,
          highContrast: this.highContrast,
          systemTheme: this.#systemTheme.value,
        })

        for (const [name, value] of Object.entries(documentState.attributes)) {
          root.setAttribute(name, value)
        }

        root.style.colorScheme = documentState.themeName

        for (const [name, value] of Object.entries(documentState.variables)) {
          root.style.setProperty(name, value)
        }
      }),
    ]
  }
}

export interface Appearance extends AppearanceState {}
