/** Events emitted by the session chat page for one-shot local UI coordination. */
export type SessionChatPageEvents = {
  scrollToPrompt: {
    direction: "next" | "previous"
  }
}
