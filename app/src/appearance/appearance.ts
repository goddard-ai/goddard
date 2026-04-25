import { Sigma } from "preact-sigma"

import { applyAppearanceSnapshot, type AppearanceMode, type AppearanceSnapshot } from "./theme.ts"

/** Public state for the app shell's appearance model. */
export type AppearanceState = {
  mode: AppearanceMode
  highContrast: boolean
  systemTheme: AppearanceSnapshot["systemTheme"]
}

export class Appearance extends Sigma<AppearanceState> {
  constructor(initialSnapshot: AppearanceSnapshot) {
    super({
      mode: initialSnapshot.mode,
      highContrast: initialSnapshot.highContrast,
      systemTheme: initialSnapshot.systemTheme,
    })
  }

  get effectiveTheme() {
    return this.mode === "system" ? this.systemTheme : this.mode
  }

  setMode(mode: AppearanceMode) {
    this.mode = mode
    this.#applyAppearance()
  }

  setHighContrast(highContrast: boolean) {
    this.highContrast = highContrast
    this.#applyAppearance()
  }

  syncSystemTheme(systemTheme: AppearanceSnapshot["systemTheme"]) {
    this.systemTheme = systemTheme

    if (this.mode === "system") {
      applyAppearanceSnapshot({
        mode: this.mode,
        highContrast: this.highContrast,
        systemTheme,
      })
    }
  }

  onSetup() {
    applyAppearanceSnapshot({
      mode: this.mode,
      highContrast: this.highContrast,
      systemTheme: this.systemTheme,
    })

    return []
  }

  #applyAppearance() {
    applyAppearanceSnapshot({
      mode: this.mode,
      highContrast: this.highContrast,
      systemTheme: this.systemTheme,
    })
  }
}

export interface Appearance extends AppearanceState {}
