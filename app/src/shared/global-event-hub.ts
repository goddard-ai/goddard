import { SigmaTarget } from "preact-sigma"

import type { AppCommandId } from "./app-commands.ts"
import type { DebugMenuSurface } from "./debug-menu.ts"

/** Shared typed global events dispatched across the active webview. */
export type GlobalEvents = {
  appMenu: { command: AppCommandId }
  commandDialogActivated: { dialogId: string }
  debugMenu: { surface: DebugMenuSurface }
}

/** One supported global event name. */
export type GlobalEventName = keyof GlobalEvents

/** Payload for one supported global event name. */
export type GlobalEventDetail<Name extends GlobalEventName> = GlobalEvents[Name]

/** One app-wide global event dispatched through the shared singleton hub. */
export type GlobalEventEnvelope<Name extends GlobalEventName = GlobalEventName> = {
  [EventName in Name]: {
    name: EventName
    detail: GlobalEventDetail<EventName>
  }
}[Name]

class GlobalEventHub extends SigmaTarget<GlobalEvents> {
  dispatch(event: GlobalEventEnvelope) {
    switch (event.name) {
      case "appMenu":
        this.emit("appMenu", event.detail)
        return
      case "commandDialogActivated":
        this.emit("commandDialogActivated", event.detail)
        return
      case "debugMenu":
        this.emit("debugMenu", event.detail)
        return
    }
  }
}

export const globalEventHub = new GlobalEventHub()

/** Dispatches one typed global event on the singleton hub. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  globalEventHub.dispatch(event)
}
