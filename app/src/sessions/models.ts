export type SessionTranscriptMessage = {
  id: string
  role: "assistant" | "user" | "system"
  authorName: string
  timestampLabel: string
  text: string
}
