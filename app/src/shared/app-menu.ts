import {
  createGlobalEventDetailListener,
  createGlobalEventDispatchScript,
} from "./global-event-hub.ts"

/** Native menu actions that the Bun host can dispatch into the active webview. */
export type AppMenuAction = "closeTab"

/** Custom browser event name dispatched when the native menu requests one app action. */
export const APP_MENU_EVENT_NAME = "goddard:app-menu"

/** Detail payload carried by one native app-menu event. */
export type AppMenuEventDetail = {
  action: AppMenuAction
}

/** Serializes one native menu action into JavaScript that Bun can inject into the webview. */
export function createAppMenuDispatchScript(detail: AppMenuEventDetail): string {
  return createGlobalEventDispatchScript(APP_MENU_EVENT_NAME, detail)
}

/** Adapts one app-menu detail listener to the EventTarget callback shape used by the shared event hub. */
export function createAppMenuEventListener(listener: (detail: AppMenuEventDetail) => void) {
  return createGlobalEventDetailListener(listener)
}
