import { SigmaType } from "preact-sigma"

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

export const Appearance = new SigmaType<AppearanceShape>("Appearance")
  .defaultState({
    mode: "system",
    highContrast: false,
    systemTheme: "light",
  })
  .computed({
    effectiveTheme() {
      return this.mode === "system" ? this.systemTheme : this.mode
    },
  })
  .actions({
    setMode(mode: AppearanceMode) {
      this.mode = mode
      persistAndApplyAppearance(this)
    },

    setHighContrast(highContrast: boolean) {
      this.highContrast = highContrast
      persistAndApplyAppearance(this)
    },

    syncSystemTheme(systemTheme: AppearanceSnapshot["systemTheme"]) {
      this.systemTheme = systemTheme

      if (this.mode === "system") {
        applyAppearanceSnapshot({
          mode: this.mode,
          highContrast: this.highContrast,
          systemTheme,
        })
      }
    },
  })
  .setup(function (initialSnapshot: AppearanceSnapshot) {
    this.act(function () {
      this.mode = initialSnapshot.mode
      this.highContrast = initialSnapshot.highContrast
      this.systemTheme = initialSnapshot.systemTheme
    })

    applyAppearanceSnapshot(initialSnapshot)

    return []
  })

export interface Appearance extends InstanceType<typeof Appearance> {}
