/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "SessionChatTranscript" | "Terminal"

/** Custom browser event name dispatched when the native debug menu requests a surface. */
export const DEBUG_MENU_EVENT_NAME = "goddard:debug-menu"

/** Detail payload carried by one native debug-menu event. */
export type DebugMenuEventDetail = {
  surface: DebugMenuSurface
}
