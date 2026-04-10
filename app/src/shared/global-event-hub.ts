import { SigmaType } from "preact-sigma"
import { APP_MENU_EVENT_NAME, type AppMenuEventDetail } from "./app-menu.ts"
import { DEBUG_MENU_EVENT_NAME, type DebugMenuEventDetail } from "./debug-menu.ts"

type GlobalEventHubShape = {
  eventVersion: number
}

type GlobalEventMap = {
  [APP_MENU_EVENT_NAME]: AppMenuEventDetail
  [DEBUG_MENU_EVENT_NAME]: DebugMenuEventDetail
}

/** One Bun-to-webview global event dispatched through the shared Electrobun bridge. */
export type GlobalEventEnvelope =
  | {
      name: typeof APP_MENU_EVENT_NAME
      detail: AppMenuEventDetail
    }
  | {
      name: typeof DEBUG_MENU_EVENT_NAME
      detail: DebugMenuEventDetail
    }

export const GlobalEventHub = new SigmaType<GlobalEventHubShape, GlobalEventMap>("GlobalEventHub")
  .defaultState({
    eventVersion: 0,
  })
  .actions({
    dispatchAppMenu(detail: AppMenuEventDetail) {
      this.eventVersion += 1
      this.commit()
      this.emit(APP_MENU_EVENT_NAME, detail)
    },
    dispatchDebugMenu(detail: DebugMenuEventDetail) {
      this.eventVersion += 1
      this.commit()
      this.emit(DEBUG_MENU_EVENT_NAME, detail)
    },
  })

export interface GlobalEventHub extends InstanceType<typeof GlobalEventHub> {}

export const globalEventHub: GlobalEventHub = new GlobalEventHub()

/** Dispatches one typed global event on the singleton hub. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  switch (event.name) {
    case APP_MENU_EVENT_NAME:
      globalEventHub.dispatchAppMenu(event.detail)
      return
    case DEBUG_MENU_EVENT_NAME:
      globalEventHub.dispatchDebugMenu(event.detail)
      return
  }
}
