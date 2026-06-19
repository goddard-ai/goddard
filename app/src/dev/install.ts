import type { Protected } from "preact-sigma"
import {
  defineLaunchableState,
  mountStateLauncher,
  registerLaunchableState,
  type LaunchCleanup,
} from "state-launcher"

import { getInboxListRequest } from "~/inbox/queries.ts"
import { queryClient } from "~/lib/query.ts"
import type { MainTab } from "~/main-tab.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import type { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import {
  blockedSession,
  blockedSessionChangesResponse,
  blockedSessionHistoryResponse,
  blockedSessionResponse,
  blockedSessionWorktreeResponse,
  criticalSessionsResponse,
  inboxAttentionResponse,
  reviewPullRequestResponse,
} from "./query-results.ts"

type LaunchableStateDeps = {
  mainTab: Protected<MainTab>
  workbenchTabSet: Protected<WorkbenchTabSet>
}

type LauncherContext = LaunchableStateDeps & {
  closeLauncher: () => void
}

let activeInstallCleanup: (() => void) | null = null

function isLaunchableStateShortcut(event: KeyboardEvent) {
  return (
    // macOS Option changes `event.key`; `event.code` still identifies the physical L key.
    (event.code === "KeyL" || event.key.toLowerCase() === "l") &&
    event.altKey &&
    event.shiftKey &&
    (event.metaKey || event.ctrlKey) &&
    !event.repeat
  )
}

function isStateLauncherKeyEvent(event: KeyboardEvent) {
  return event.composedPath().some((target) => {
    return (
      target instanceof HTMLElement &&
      (target.hasAttribute("data-state-launcher") ||
        target.hasAttribute("data-state-launcher-host"))
    )
  })
}

function composeCleanups(cleanups: LaunchCleanup[]) {
  return async () => {
    for (let index = cleanups.length - 1; index >= 0; index -= 1) {
      await cleanups[index]?.()
    }
  }
}

function defineLaunchableStates({ closeLauncher, mainTab, workbenchTabSet }: LauncherContext) {
  const inboxAttentionQueue = defineLaunchableState("inbox.attentionQueue", {
    label: "Inbox attention queue",
    description: "Unread session blockers and a pull-request update.",
    tags: ["inbox", "session", "pull request", "attention"],
    launch() {
      const cleanup = composeCleanups([
        queryClient.injectData(
          goddardSdk.inbox.list,
          [getInboxListRequest()],
          inboxAttentionResponse,
        ),
        queryClient.injectData(
          goddardSdk.pr.get,
          [{ id: reviewPullRequestResponse.pullRequest.id }],
          reviewPullRequestResponse,
        ),
        queryClient.injectData(
          goddardSdk.session.get,
          [{ id: blockedSession.id }],
          blockedSessionResponse,
        ),
        queryClient.injectData(
          goddardSdk.session.history,
          [{ id: blockedSession.id }],
          blockedSessionHistoryResponse,
        ),
      ])
      workbenchTabSet.activateTab("main")
      mainTab.selectKind("inbox")
      closeLauncher()
      return cleanup
    },
  })

  const sessionsCriticalQueue = defineLaunchableState("sessions.criticalQueue", {
    label: "Sessions critical queue",
    description: "Active, blocked, failed, and completed sessions together.",
    tags: ["sessions", "triage", "blocked", "error"],
    launch() {
      const cleanup = composeCleanups([
        queryClient.injectData(
          goddardSdk.session.list,
          [{ limit: SESSION_LIST_LIMIT }],
          criticalSessionsResponse,
        ),
      ])
      workbenchTabSet.activateTab("main")
      mainTab.selectKind("sessions")
      closeLauncher()
      return cleanup
    },
  })

  const sessionBlockedWithChanges = defineLaunchableState("session.blockedWithChanges", {
    label: "Blocked session with changes",
    description: "Session detail with a pending permission request and seeded workspace diff.",
    tags: ["session", "blocked", "permission", "diff"],
    launch() {
      const cleanup = composeCleanups([
        queryClient.injectData(
          goddardSdk.session.get,
          [{ id: blockedSession.id }],
          blockedSessionResponse,
        ),
        queryClient.injectData(
          goddardSdk.session.history,
          [{ id: blockedSession.id }],
          blockedSessionHistoryResponse,
        ),
        queryClient.injectData(
          goddardSdk.session.worktree.get,
          [{ id: blockedSession.id }],
          blockedSessionWorktreeResponse,
        ),
        queryClient.injectData(
          goddardSdk.session.changes,
          [{ id: blockedSession.id }],
          blockedSessionChangesResponse,
        ),
      ])
      workbenchTabSet.openOrFocusTab({
        kind: "sessionChat",
        props: {
          relatedFilesystemPath: blockedSession.cwd,
          sessionId: blockedSession.id,
          sessionTitle: blockedSession.title,
        },
      })
      closeLauncher()
      return cleanup
    },
  })

  return [inboxAttentionQueue, sessionsCriticalQueue, sessionBlockedWithChanges]
}

export function installLaunchableStates(deps: LaunchableStateDeps) {
  activeInstallCleanup?.()

  let closeLauncher = () => {}
  const commands = defineLaunchableStates({
    ...deps,
    closeLauncher: () => {
      closeLauncher()
    },
  })
  const unregisterStates = registerLaunchableState(commands)
  const launcher = mountStateLauncher({
    initiallyOpen: false,
    position: "bottom-right",
    title: "App states",
  })
  closeLauncher = () => {
    launcher.close()
  }
  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape" && isStateLauncherKeyEvent(event)) {
      event.preventDefault()
      event.stopPropagation()
      launcher.close()
      return
    }

    if (!isLaunchableStateShortcut(event)) {
      return
    }

    event.preventDefault()
    event.stopPropagation()
    launcher.toggle()
  }

  document.addEventListener("keydown", handleKeyDown, true)

  activeInstallCleanup = () => {
    document.removeEventListener("keydown", handleKeyDown, true)
    unregisterStates()
    launcher.unmount()
    activeInstallCleanup = null
  }

  return activeInstallCleanup
}

import.meta.hot?.dispose(() => {
  activeInstallCleanup?.()
})
