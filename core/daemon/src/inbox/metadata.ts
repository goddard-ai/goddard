import type { DaemonSession, InboxHeadline, InboxScope } from "@goddard-ai/schema/daemon"

const genericScopes = new Set(["task", "update", "work", "work in progress", "progress"])

function normalizeText(value: string | null | undefined, maxLength: number) {
  const normalized = value?.trim().replace(/\s+/g, " ") ?? ""
  return normalized.length > maxLength ? normalized.slice(0, maxLength).trimEnd() : normalized
}

function normalizeScope(value: string | null | undefined) {
  const normalized = normalizeText(value, 80)
  if (!normalized || genericScopes.has(normalized.toLowerCase())) {
    return null
  }

  return normalized as InboxScope
}

function normalizeHeadline(value: string | null | undefined) {
  const normalized = normalizeText(value, 120).replace(/^I\s+(?:am\s+)?/i, "")
  return normalized ? (normalized as InboxHeadline) : null
}

function fallbackScope(
  session: Pick<DaemonSession, "inboxScope" | "initiative" | "title" | "titleState">,
) {
  const title = session.titleState === "placeholder" ? null : session.title

  return (
    normalizeScope(session.inboxScope) ??
    normalizeScope(session.initiative) ??
    normalizeScope(title) ??
    ("Session" as InboxScope)
  )
}

function fallbackHeadline(input: {
  headline?: string | null
  blockedReason?: string | null
  fallback?: string | null
}) {
  return (
    normalizeHeadline(input.headline) ??
    normalizeHeadline(input.blockedReason) ??
    normalizeHeadline(input.fallback) ??
    ("Attention needed" as InboxHeadline)
  )
}

/** Resolves agent-supplied inbox metadata into stable, bounded row preview text. */
export function resolveInboxMetadata(input: {
  session: Pick<
    DaemonSession,
    "inboxScope" | "initiative" | "title" | "titleState" | "blockedReason"
  >
  scope?: string | null
  headline?: string | null
  fallbackHeadline?: string | null
}) {
  const suppliedScope = normalizeScope(input.scope)

  return {
    scope: suppliedScope ?? fallbackScope(input.session),
    headline: fallbackHeadline({
      headline: input.headline,
      blockedReason: input.session.blockedReason,
      fallback: input.fallbackHeadline,
    }),
    suppliedScope,
  }
}
