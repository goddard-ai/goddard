import { Sigma } from "preact-sigma"

/** Suggested binding shown before the user opts into the native global shortcut. */
export const GLOBAL_SESSION_LAUNCH_SHORTCUT_DEFAULT_BINDING = "Command+Period"

/** Native registration state for the opt-in global session launch shortcut. */
export type GlobalSessionLaunchShortcutRegistrationState =
  | { status: "unregistered"; error: null }
  | { status: "registered"; error: null }
  | { status: "unavailable"; error: string }

/** Public app-owned state for the opt-in global session launch shortcut. */
export type GlobalSessionLaunchShortcutState = {
  binding: string
  enabled: boolean
  registration: GlobalSessionLaunchShortcutRegistrationState
}

function createUnregisteredState(): GlobalSessionLaunchShortcutRegistrationState {
  return {
    status: "unregistered",
    error: null,
  }
}

function normalizeGlobalShortcutBinding(binding: string) {
  const trimmedBinding = binding.trim()
  return trimmedBinding.length > 0 ? trimmedBinding : GLOBAL_SESSION_LAUNCH_SHORTCUT_DEFAULT_BINDING
}

/** Owns the persisted opt-in state and native registration status for global session launch. */
export class GlobalSessionLaunchShortcut extends Sigma<GlobalSessionLaunchShortcutState> {
  constructor() {
    super({
      binding: GLOBAL_SESSION_LAUNCH_SHORTCUT_DEFAULT_BINDING,
      enabled: false,
      registration: createUnregisteredState(),
    })
  }

  get registrationError() {
    return this.registration.status === "unavailable" ? this.registration.error : null
  }

  canEnable(projectCount: number) {
    return projectCount > 0
  }

  enable(projectCount: number) {
    if (!this.canEnable(projectCount)) {
      return false
    }

    this.enabled = true
    this.registration = createUnregisteredState()
    return true
  }

  disable() {
    this.enabled = false
    this.registration = createUnregisteredState()
  }

  setBinding(binding: string) {
    this.binding = normalizeGlobalShortcutBinding(binding)
    this.registration = createUnregisteredState()
  }

  markRegistered() {
    this.registration = {
      status: "registered",
      error: null,
    }
  }

  markRegistrationUnavailable(error: string) {
    this.enabled = false
    this.registration = {
      status: "unavailable",
      error,
    }
  }

  markUnregistered() {
    this.registration = createUnregisteredState()
  }
}

export interface GlobalSessionLaunchShortcut extends GlobalSessionLaunchShortcutState {}
