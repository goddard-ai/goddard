/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "SessionChatDebug" | "Terminal"

/** Complete debug surface table used by the native development menu. */
export const DebugMenuSurfaces = {
  SessionChatDebug: "SessionChatDebug",
  Terminal: "Terminal",
} as const satisfies Record<DebugMenuSurface, DebugMenuSurface>
