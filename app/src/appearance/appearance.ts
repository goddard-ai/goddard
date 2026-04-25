import { Sigma } from "preact-sigma"

import {
  applyAppearanceSnapshot,
  writeAppearancePreferences,
  type AppearanceMode,
  type AppearanceSnapshot,
} from "./theme.ts"

type AppearanceState = {
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
    this.#persistAndApplyAppearance()
  }

  setHighContrast(highContrast: boolean) {
    this.highContrast = highContrast
    this.#persistAndApplyAppearance()
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

  #persistAndApplyAppearance() {
    writeAppearancePreferences(this.mode, this.highContrast)
    applyAppearanceSnapshot({
      mode: this.mode,
      highContrast: this.highContrast,
      systemTheme: this.systemTheme,
    })
  }
}

export interface Appearance extends AppearanceState {}
