/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "SessionChatTranscript" | "Terminal"

/** Complete debug surface table used by the native development menu. */
export const DebugMenuSurfaces = {
  SessionChatTranscript: "SessionChatTranscript",
  Terminal: "Terminal",
} as const satisfies Record<DebugMenuSurface, DebugMenuSurface>

/** Payload carried by one native debug-menu event. */
export type DebugMenuEventDetail = {
  surface: DebugMenuSurface
}
