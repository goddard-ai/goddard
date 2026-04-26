import { signal } from "@preact/signals"
import { Sigma } from "preact-sigma"

import {
  applyAppearanceSnapshot,
  readSystemThemeName,
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

  syncSystemTheme(systemTheme: BuiltInThemeName) {
    if (this.#systemTheme.value === systemTheme) {
      return
    }

    this.#systemTheme.value = systemTheme

    if (this.mode === "system") {
      this.#applyAppearance()
    }
  }

  /** Applies the current appearance preferences to the document without changing persisted state. */
  applyDocumentAppearance() {
    this.#applyAppearance()
  }

  onSetup() {
    this.#applyAppearance()

    return []
  }

  #applyAppearance() {
    applyAppearanceSnapshot({
      mode: this.mode,
      highContrast: this.highContrast,
      systemTheme: this.#systemTheme.value,
    })
  }
}

export interface Appearance extends AppearanceState {}
