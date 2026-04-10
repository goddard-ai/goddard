/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "SessionChatTranscript" | "Terminal"

/** Payload carried by one native debug-menu event. */
export type DebugMenuEventDetail = {
  surface: DebugMenuSurface
}
