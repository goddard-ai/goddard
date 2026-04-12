/** One ordered content block rendered inside a transcript message bubble. */
export type SessionTranscriptContentBlock =
  | {
      type: "text"
      text: string
    }
  | {
      type: "resource_link"
      name: string
      uri: string
      title: string | null
      description: string | null
    }

/** ACP tool kinds the session transcript currently renders. */
export type SessionTranscriptToolKind =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "execute"
  | "think"
  | "fetch"
  | "switch_mode"
  | "other"

/** ACP tool statuses the session transcript currently renders. */
export type SessionTranscriptToolStatus = "pending" | "in_progress" | "completed" | "failed"

/** One normalized tool-call content payload shown inside a transcript tool row. */
export type SessionTranscriptToolContent =
  | {
      type: "content"
      text: string | null
    }
  | {
      type: "diff"
      path: string | null
      oldText: string | null
      newText: string | null
    }
  | {
      type: "terminal"
      terminalId: string
    }

/** One file location associated with a transcript tool row. */
export type SessionTranscriptToolLocation = {
  path: string
  line: number | null
}

/** One text-oriented transcript row rendered as a chat bubble. */
export type SessionTranscriptTextMessage = {
  kind: "message"
  id: string
  role: "assistant" | "user" | "system"
  authorName: string
  timestampLabel: string
  content: readonly SessionTranscriptContentBlock[]
  streaming?: boolean
}

/** One normalized tool-call transcript row rendered as a dedicated tool card. */
export type SessionTranscriptToolCall = {
  kind: "toolCall"
  id: string
  toolCallId: string
  authorName: string
  timestampLabel: string
  title: string
  toolKind: SessionTranscriptToolKind
  status: SessionTranscriptToolStatus
  content: readonly SessionTranscriptToolContent[]
  locations: readonly SessionTranscriptToolLocation[]
}

/** One transcript row shown inside the session chat surface. */
export type SessionTranscriptItem = SessionTranscriptTextMessage | SessionTranscriptToolCall
