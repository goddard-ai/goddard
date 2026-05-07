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

/** ACP permission option kinds the session transcript can present back to the agent. */
export type SessionTranscriptPermissionOptionKind =
  | "allow_once"
  | "allow_always"
  | "reject_once"
  | "reject_always"

/** One selectable ACP permission option attached to a permission request row. */
export type SessionTranscriptPermissionOption = {
  optionId: string
  name: string
  kind: SessionTranscriptPermissionOptionKind
}

/** UI-facing resolution state for one ACP permission request row. */
export type SessionTranscriptPermissionStatus =
  | "pending"
  | "allowed"
  | "denied"
  | "resolved"
  | "cancelled"
  | "failed"

/** One transcript row that asks the user to resolve an ACP permission request. */
export type SessionTranscriptPermissionRequest = {
  kind: "permissionRequest"
  id: string
  requestId: string | number
  authorName: string
  timestampLabel: string
  title: string
  toolKind: SessionTranscriptToolKind
  status: SessionTranscriptPermissionStatus
  context: string | null
  locations: readonly SessionTranscriptToolLocation[]
  options: readonly SessionTranscriptPermissionOption[]
  selectedOptionId: string | null
  error: string | null
}

/** ACP plan step priorities the session transcript currently renders. */
export type SessionTranscriptPlanEntryPriority = "high" | "medium" | "low"

/** ACP plan step statuses the session transcript currently renders. */
export type SessionTranscriptPlanEntryStatus = "pending" | "in_progress" | "completed"

/** One normalized ACP plan step shown inside a plan update row. */
export type SessionTranscriptPlanEntry = {
  content: string
  priority: SessionTranscriptPlanEntryPriority
  status: SessionTranscriptPlanEntryStatus
}

/** One transcript row that shows the current complete ACP plan state. */
export type SessionTranscriptPlanUpdate = {
  kind: "planUpdate"
  id: string
  authorName: string
  timestampLabel: string
  title: string
  entries: readonly SessionTranscriptPlanEntry[]
}

/** Terminal lifecycle states shown after a turn stops producing transcript content. */
export type SessionTranscriptTurnStopStatus =
  | "completed"
  | "stopped"
  | "failed"
  | "cancelled"
  | "interrupted"

/** One compact transcript row that marks how a prompt turn ended. */
export type SessionTranscriptTurnStop = {
  kind: "turnStop"
  id: string
  status: SessionTranscriptTurnStopStatus
  title: string
  reason: string | null
  timestamp: string | null
}

/** One transcript row shown inside the session chat surface. */
export type SessionTranscriptItem =
  | SessionTranscriptTextMessage
  | SessionTranscriptToolCall
  | SessionTranscriptPermissionRequest
  | SessionTranscriptPlanUpdate
  | SessionTranscriptTurnStop
