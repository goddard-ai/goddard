import { Sigma } from "preact-sigma"

import {
  applyAppearanceSnapshot,
  writeAppearancePreferences,
  type AppearanceMode,
  type AppearanceSnapshot,
} from "./theme.ts"

type AppearanceShape = {
  mode: AppearanceMode
  highContrast: boolean
  systemTheme: AppearanceSnapshot["systemTheme"]
}

function persistAndApplyAppearance(state: AppearanceShape) {
  writeAppearancePreferences(state.mode, state.highContrast)
  applyAppearanceSnapshot({
    mode: state.mode,
    highContrast: state.highContrast,
    systemTheme: state.systemTheme,
  })
}

export class Appearance extends Sigma<AppearanceShape> {
  declare mode: AppearanceMode
  declare highContrast: boolean
  declare systemTheme: AppearanceSnapshot["systemTheme"]

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
    persistAndApplyAppearance(this)
  }

  setHighContrast(highContrast: boolean) {
    this.highContrast = highContrast
    persistAndApplyAppearance(this)
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
}
