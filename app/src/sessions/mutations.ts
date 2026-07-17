import type {
  CreateSessionRequest,
  DaemonSession,
  MergeSessionWorktreeRequest,
  RemoveSessionProfileRequest,
  SessionPermissionResponseRequest,
  SessionPromptRequest,
  SetSessionConfigOptionRequest,
  SetSessionModelRequest,
  SetSessionWorktreeMergeTargetBranchRequest,
  SetSessionProfileRequest,
  SteerSessionRequest,
} from "@goddard-ai/sdk"

import { createMutationsProvider } from "~/lib/mutations-provider.tsx"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  invalidateSessionChanges,
  invalidateSessionLaunchPreview,
  invalidateSessionLists,
  invalidateSessionViews,
} from "./cache.ts"

export function evictSessionHistory(sessionId: DaemonSession["id"]) {
  queryClient.evict(goddardSdk.session.history, [{ id: sessionId }])
}

/**
 * Creates one session and refreshes the visible session list afterwards.
 */
export async function createSession(input: CreateSessionRequest) {
  const result = await goddardSdk.session.create(input)
  invalidateSessionLists()
  invalidateSessionLaunchPreview()
  return result
}

/**
 * Schedules one launch lease for delayed cleanup after the launch dialog stops using it.
 */
export async function releaseSessionLaunchLease(launchLeaseId: string | null | undefined) {
  if (!launchLeaseId) {
    return
  }

  await goddardSdk.session.launchLease.release({ launchLeaseId })
}

/**
 * Schedules one abandoned launch worktree for delayed cleanup.
 */
export async function releaseSessionLaunchWorktree(launchWorktreeId: string | null | undefined) {
  if (!launchWorktreeId) {
    return
  }

  await goddardSdk.session.launchWorktree.release({ launchWorktreeId })
}

/**
 * Submits one prompt into an existing session and refreshes the affected session views.
 */
export async function submitSessionPrompt(props: SessionPromptRequest) {
  await goddardSdk.session.prompt(props)
  invalidateSessionViews(props.id)
}

/**
 * Cancels the active turn, injects one replacement prompt, and refreshes views.
 */
export async function steerSessionPrompt(props: SteerSessionRequest) {
  const result = await goddardSdk.session.steer(props)
  invalidateSessionViews(props.id)
  return result
}

/**
 * Removes the newest queued prompt and refreshes affected session views.
 */
export async function popQueuedSessionPrompt(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.session.popQueuedPrompt({ id: sessionId })
  invalidateSessionViews(sessionId)
  return result
}

/**
 * Updates one active ACP session config option and refreshes the affected session views.
 */
export async function setSessionConfigOption(props: SetSessionConfigOptionRequest) {
  const result = await goddardSdk.session.configOption.set(props)
  invalidateSessionViews(props.id)
  return result
}

/**
 * Updates one active ACP session model and refreshes the affected session views.
 */
export async function setSessionModel(props: SetSessionModelRequest) {
  const result = await goddardSdk.session.model.set(props)
  invalidateSessionViews(props.id)
  return result
}

/** Replaces one global session profile and refreshes profile selectors. */
export async function setSessionProfile(props: SetSessionProfileRequest) {
  const result = await goddardSdk.session.profile.set(props)
  queryClient.invalidate(goddardSdk.session.profile.list)
  return result
}

/** Removes one global session profile and refreshes profile selectors. */
export async function removeSessionProfile(props: RemoveSessionProfileRequest) {
  const result = await goddardSdk.session.profile.remove(props)
  queryClient.invalidate(goddardSdk.session.profile.list)
  return result
}

/**
 * Responds to one ACP permission request and refreshes the affected session views.
 */
export async function respondSessionPermission(props: SessionPermissionResponseRequest) {
  await goddardSdk.session.respondPermission(props)
  invalidateSessionViews(props.id)
}

/**
 * Reconnects one loadable session and refreshes the affected session views.
 */
export async function reconnectSession(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.session.connect({ id: sessionId })
  invalidateSessionViews(sessionId)
  return result
}

/**
 * Cancels the active turn for one session and refreshes the affected session views.
 */
export async function cancelSessionTurn(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.session.cancel({ id: sessionId })
  invalidateSessionViews(sessionId)
  return result
}

/**
 * Updates one persisted worktree merge target branch and refreshes the affected session views.
 */
export async function setSessionWorktreeMergeTargetBranch(
  input: SetSessionWorktreeMergeTargetBranchRequest,
) {
  const result = await goddardSdk.session.worktree.mergeTargetBranch.set(input)
  invalidateSessionViews(input.id)
  return result
}

/**
 * Merges one session worktree into its persisted target branch and refreshes the affected views.
 */
export async function mergeSessionWorktree(input: MergeSessionWorktreeRequest) {
  const result = await goddardSdk.session.worktree.merge(input)
  invalidateSessionViews(input.id)
  if (result.merged) {
    invalidateSessionChanges(input.id)
  }
  return result
}

/**
 * Marks one session completed without shutting it down, then refreshes session and inbox views.
 */
export async function completeSession(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.inbox.completeSession({ id: sessionId })
  invalidateSessionViews(sessionId)
  queryClient.invalidate(goddardSdk.inbox.list)
  return result
}

export const SessionsPageMutations = createMutationsProvider<{
  openSession: (sessionId: DaemonSession["id"]) => void
  openSessionChanges: (sessionId: DaemonSession["id"]) => void
}>("SessionsPageMutations")
