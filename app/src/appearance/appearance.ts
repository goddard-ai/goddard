import { signal } from "@preact/signals"
import { listen, Sigma } from "preact-sigma"

import {
  buildAppearanceDocumentState,
  type AppearanceMode,
  type BuiltInThemeName,
} from "./theme.ts"

const SYSTEM_COLOR_SCHEME_MEDIA_QUERY = "(prefers-color-scheme: dark)"

/** Reads the observed system theme with a browserless fallback for tests and early bootstrap. */
function readSystemThemeName(mediaQuery?: MediaQueryList): BuiltInThemeName {
  if (mediaQuery) {
    return mediaQuery.matches ? "dark" : "light"
  }

  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "light"
  }

  return window.matchMedia(SYSTEM_COLOR_SCHEME_MEDIA_QUERY).matches ? "dark" : "light"
}

/** Public state for the app shell's appearance model. */
export type AppearanceState = {
  mode: AppearanceMode
  highContrast: boolean
}

export class Appearance extends Sigma<AppearanceState> {
  /** Tracks the browser's color-scheme outside persisted state because it is runtime-derived. */
  #systemTheme = signal(readSystemThemeName())

  constructor(initialState: AppearanceState) {
    super({
      mode: initialState.mode,
      highContrast: initialState.highContrast,
    })
  }

  get effectiveTheme() {
    return this.mode === "system" ? this.#systemTheme.value : this.mode
  }

  setMode(mode: AppearanceMode) {
    this.mode = mode
    this.#applyAppearance()
  }

  setHighContrast(highContrast: boolean) {
    this.highContrast = highContrast
    this.#applyAppearance()
  }

  /** Applies the current appearance preferences to the document without changing persisted state. */
  applyDocumentAppearance() {
    this.#applyAppearance()
  }

  onSetup() {
    this.#applyAppearance()

    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return []
    }

    const mediaQuery = window.matchMedia(SYSTEM_COLOR_SCHEME_MEDIA_QUERY)
    const syncSystemTheme = () => {
      const systemTheme = readSystemThemeName(mediaQuery)

      if (this.#systemTheme.value === systemTheme) {
        return
      }

      this.#systemTheme.value = systemTheme

      if (this.mode === "system") {
        this.#applyAppearance()
      }
    }

    syncSystemTheme()

    return [listen(mediaQuery, "change", syncSystemTheme)]
  }

  #applyAppearance() {
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
  }
}

export interface Appearance extends AppearanceState {}
