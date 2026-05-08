/** Debug surfaces that the native development menu can request inside the webview. */
export type DebugMenuSurface = "Inbox" | "SessionChatTranscript" | "Terminal"

/** Complete debug surface table used by the native development menu. */
export const DebugMenuSurfaces = {
  Inbox: "Inbox",
  SessionChatTranscript: "SessionChatTranscript",
  Terminal: "Terminal",
} as const satisfies Record<DebugMenuSurface, DebugMenuSurface>
