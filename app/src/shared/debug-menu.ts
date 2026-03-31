/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "terminal"

/** Custom browser event name dispatched when the native debug menu requests a surface. */
export const DEBUG_MENU_EVENT_NAME = "goddard:debug-menu"

/** Detail payload carried by one native debug-menu event. */
export type DebugMenuEventDetail = {
  surface: DebugMenuSurface
}

/** Serializes one debug-menu request into JavaScript that Bun can inject into the webview. */
export function createDebugMenuDispatchScript(detail: DebugMenuEventDetail): string {
  return `window.dispatchEvent(new CustomEvent(${JSON.stringify(DEBUG_MENU_EVENT_NAME)}, { detail: ${JSON.stringify(detail)} }));`
}
