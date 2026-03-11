export const SessionStatus = [
  "idle",
  "active",
  "archived",
  "blocked",
  "done",
  "error",
  "cancelled",
] as const
export type SessionStatus = (typeof SessionStatus)[number]
