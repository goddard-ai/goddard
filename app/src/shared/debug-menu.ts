import {
  createGlobalEventDetailListener,
  createGlobalEventDispatchScript,
} from "./global-event-hub.ts"

/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "SessionChatTranscript" | "Terminal"

/** Custom browser event name dispatched when the native debug menu requests a surface. */
export const DEBUG_MENU_EVENT_NAME = "goddard:debug-menu"

/** Detail payload carried by one native debug-menu event. */
export type DebugMenuEventDetail = {
  surface: DebugMenuSurface
}

/** Serializes one debug-menu request into JavaScript that Bun can inject into the webview. */
export function createDebugMenuDispatchScript(detail: DebugMenuEventDetail): string {
  return createGlobalEventDispatchScript(DEBUG_MENU_EVENT_NAME, detail)
}

/** Adapts one debug-menu detail listener to the EventTarget callback shape used by the shared event hub. */
export function createDebugMenuEventListener(listener: (detail: DebugMenuEventDetail) => void) {
  return createGlobalEventDetailListener(listener)
}
