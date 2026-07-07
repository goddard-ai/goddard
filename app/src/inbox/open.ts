import type { InboxItem } from "@goddard-ai/inbox/schema"

import { queryClient } from "~/lib/query.ts"
import { getPullRequestDisplayTitle } from "~/pull-requests/display.ts"
import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle } from "~/sessions/display.ts"
import { warmWorkbenchTab, warmWorkbenchTabOnIdle } from "~/workbench-tab-registry.ts"
import type { WorkbenchOpenTabInput, WorkbenchTabSet } from "~/workbench-tab-set.ts"
import { isInboxEntityKind } from "./entity-kind.ts"

export type PreparedInboxWorkbenchTarget = {
  entityId: InboxItem["entityId"]
  itemId: InboxItem["id"]
  tab: WorkbenchOpenTabInput<"sessionChat" | "pullRequest">
  updatedAt: InboxItem["updatedAt"]
}

function isPreparedInboxWorkbenchTargetCurrent(
  item: InboxItem,
  target: PreparedInboxWorkbenchTarget,
) {
  return (
    target.itemId === item.id &&
    target.entityId === item.entityId &&
    target.updatedAt === item.updatedAt
  )
}

/** Resolves and warms the workbench tab one inbox row would open, without activating it. */
export async function prepareInboxItemWorkbenchTarget(
  item: InboxItem,
): Promise<PreparedInboxWorkbenchTarget | null> {
  if (isInboxEntityKind(item, "session")) {
    const { session } = await queryClient.prefetch(
      goddardSdk.session.get,
      [{ id: item.entityId }],
      { force: true },
    )

    const tab = {
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: session.cwd,
        sessionId: session.id,
        sessionTitle: getSessionDisplayTitle(session),
      },
    } satisfies WorkbenchOpenTabInput<"sessionChat">

    await warmWorkbenchTab(tab)

    return {
      entityId: item.entityId,
      itemId: item.id,
      tab,
      updatedAt: item.updatedAt,
    }
  }

  if (!isInboxEntityKind(item, "pullRequest")) {
    return null
  }

  const { pullRequest } = await queryClient.prefetch(goddardSdk.pr.get, [{ id: item.entityId }], {
    force: true,
  })

  const tab = {
    kind: "pullRequest",
    props: {
      relatedFilesystemPath: pullRequest.cwd,
      pullRequestId: pullRequest.id,
      pullRequestTitle: getPullRequestDisplayTitle(pullRequest),
    },
  } satisfies WorkbenchOpenTabInput<"pullRequest">

  await warmWorkbenchTab(tab)

  return {
    entityId: item.entityId,
    itemId: item.id,
    tab,
    updatedAt: item.updatedAt,
  }
}

/** Opens the workbench tab linked to one daemon inbox row. */
export async function openInboxItemInWorkbench(input: {
  closeCurrentCleanTab?: boolean
  item: InboxItem
  preparedTarget?: PreparedInboxWorkbenchTarget | null
  workbenchTabSet: Pick<WorkbenchTabSet, "activeTabId" | "closeTabIfClean" | "openOrFocusTab">
}) {
  const previousActiveTabId = input.workbenchTabSet.activeTabId
  const target =
    input.preparedTarget && isPreparedInboxWorkbenchTargetCurrent(input.item, input.preparedTarget)
      ? input.preparedTarget
      : await prepareInboxItemWorkbenchTarget(input.item)

  if (!target) {
    return
  }

  const openedTab = input.workbenchTabSet.openOrFocusTab(target.tab)

  if (input.closeCurrentCleanTab === true && previousActiveTabId !== openedTab.id) {
    input.workbenchTabSet.closeTabIfClean(previousActiveTabId)
  }
}

/** Warms heavier resources for one already prepared inbox target after idle time. */
export async function warmPreparedInboxWorkbenchTargetOnIdle(target: PreparedInboxWorkbenchTarget) {
  await warmWorkbenchTabOnIdle(target.tab)
}
