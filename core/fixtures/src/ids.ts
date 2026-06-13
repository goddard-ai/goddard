import type { InboxItemId } from "@goddard-ai/inbox/schema"
import type { PullRequestId } from "@goddard-ai/pull-request/schema"
import type { SessionId } from "@goddard-ai/session/schema"

type FixtureIdPrefix = "ses" | "inb" | "pr" | "turn" | "req" | "tool" | "wt"

function normalizeIdPart(value: string | number) {
  return String(value)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
}

export function fixtureId(prefix: FixtureIdPrefix, value: string | number) {
  const suffix = normalizeIdPart(value)

  return `${prefix}_${suffix || "fixture"}`
}

export function fixtureSessionId(value: string | number): SessionId {
  return fixtureId("ses", value) as SessionId
}

export function fixtureInboxItemId(value: string | number): InboxItemId {
  return fixtureId("inb", value) as InboxItemId
}

export function fixturePullRequestId(value: string | number): PullRequestId {
  return fixtureId("pr", value) as PullRequestId
}
