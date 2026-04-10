import { SigmaTarget } from "preact-sigma"
import type { AppMenuEventDetail } from "./app-menu.ts"
import type { DebugMenuEventDetail } from "./debug-menu.ts"

/** Shared typed global events dispatched from the Bun host into the active webview. */
export type GlobalEvents = {
  appMenu: AppMenuEventDetail
  debugMenu: DebugMenuEventDetail
}

/** One supported global event name. */
export type GlobalEventName = keyof GlobalEvents

/** Payload for one supported global event name. */
export type GlobalEventDetail<Name extends GlobalEventName> = GlobalEvents[Name]

/** One Bun-to-webview global event dispatched through the shared Electrobun bridge. */
export type GlobalEventEnvelope<Name extends GlobalEventName = GlobalEventName> = {
  [EventName in Name]: {
    name: EventName
    detail: GlobalEventDetail<EventName>
  }
}[Name]

export const globalEventHub = new SigmaTarget<GlobalEvents>()

/** Dispatches one typed global event on the singleton hub. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  globalEventHub.emit(event.name, event.detail)
}
