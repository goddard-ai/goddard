import { SigmaTarget } from "preact-sigma"

import type { AppMenuEventDetail } from "./app-menu.ts"
import type { DebugMenuEventDetail } from "./debug-menu.ts"

/** Payload carried by one app-wide session launch dialog request. */
export type SessionLaunchDialogRequestDetail = {
  preferredProjectPath: string | null
}

/** Shared typed global events dispatched across the active webview. */
export type GlobalEvents = {
  appMenu: AppMenuEventDetail
  debugMenu: DebugMenuEventDetail
  sessionLaunchDialogRequested: SessionLaunchDialogRequestDetail
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

export const globalEventHub = new SigmaTarget<GlobalEvents>()

/** Dispatches one typed global event on the singleton hub. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  globalEventHub.emit(event.name, event.detail)
}

/** Requests that the shared session launch dialog open with an optional project preselection. */
export function requestSessionLaunchDialog(preferredProjectPath?: string | null) {
  dispatchGlobalEvent({
    name: "sessionLaunchDialogRequested",
    detail: {
      preferredProjectPath: preferredProjectPath ?? null,
    },
  })
}
