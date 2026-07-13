import {
  deriveSessionProfileConfig,
  type AgentSessionProfiles,
  type SessionProfile,
  type SessionProfileId,
} from "@goddard-ai/sdk"
import { t } from "@lingui/core/macro"

export const sessionProfileIds = [
  "routine",
  "debug",
  "deep",
] as const satisfies readonly SessionProfileId[]

/** Returns the localized label for one fixed session-profile slot. */
export function getSessionProfileLabel(profileId: SessionProfileId) {
  switch (profileId) {
    case "routine":
      return t`Routine`
    case "debug":
      return t`Debug`
    case "deep":
      return t`Deep`
  }
}

type SessionProfileConfig = ReturnType<typeof deriveSessionProfileConfig>

type SessionProfileSelection = {
  modelId: string | null
  thinkingValue: string | null
  approvalModeValue: string | null
}

function profilesEqual(left: SessionProfile, right: SessionProfile) {
  return (
    left.model === right.model &&
    left.thoughtLevel === right.thoughtLevel &&
    left.approvalMode === right.approvalMode
  )
}

/** Returns configured profiles that still resolve against the current ACP option contract. */
export function getAvailableSessionProfiles(
  profiles: AgentSessionProfiles | null | undefined,
  config: SessionProfileConfig,
) {
  return sessionProfileIds.flatMap((profileId) => {
    const profile = profiles?.[profileId]
    if (!profile || config.resolveProfile(profile).status !== "available") {
      return []
    }

    return [{ profileId, profile }]
  })
}

/** Derives the matching fixed profile from effective model, thinking, and approval selections. */
export function findMatchingSessionProfileId(
  profiles: AgentSessionProfiles | null | undefined,
  config: SessionProfileConfig,
  selection?: SessionProfileSelection,
) {
  if (selection) {
    const effectiveProfile = config.createProfile(selection)
    if (!effectiveProfile) {
      return null
    }

    return (
      sessionProfileIds.find((profileId) => {
        const profile = profiles?.[profileId]
        return profile ? profilesEqual(profile, effectiveProfile) : false
      }) ?? null
    )
  }

  return (
    sessionProfileIds.find((profileId) => {
      const profile = profiles?.[profileId]
      return profile ? config.matchesProfile(profile) : false
    }) ?? null
  )
}
