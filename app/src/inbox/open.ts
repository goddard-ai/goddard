import type { InboxItem } from "@goddard-ai/inbox/schema"

import { queryClient } from "~/lib/query.ts"
import { getPullRequestDisplayTitle } from "~/pull-requests/display.ts"
import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle } from "~/sessions/display.ts"
import { preloadWorkbenchTabComponent } from "~/workbench-tab-registry.ts"
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

    await Promise.all([
      queryClient.prefetch(goddardSdk.session.history, [{ id: session.id }], {
        force: true,
        refetchOnWindowReactivate: false,
      }),
      queryClient.prefetch(goddardSdk.session.worktree.get, [{ id: session.id }], {
        force: true,
      }),
      queryClient.prefetch(goddardSdk.adapter.list, [
        { cwd: session.cwd, includeUninstalled: true },
      ]),
      preloadWorkbenchTabComponent(tab.kind),
    ])

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

  await preloadWorkbenchTabComponent(tab.kind)

  return {
    entityId: item.entityId,
    itemId: item.id,
    tab,
    updatedAt: item.updatedAt,
  }
}

/** Opens the workbench tab linked to one daemon inbox row. */
export async function openInboxItemInWorkbench(input: {
  item: InboxItem
  preparedTarget?: PreparedInboxWorkbenchTarget | null
  workbenchTabSet: Pick<WorkbenchTabSet, "openOrFocusTab">
}) {
  const target =
    input.preparedTarget && isPreparedInboxWorkbenchTargetCurrent(input.item, input.preparedTarget)
      ? input.preparedTarget
      : await prepareInboxItemWorkbenchTarget(input.item)

  if (!target) {
    return
  }

  input.workbenchTabSet.openOrFocusTab(target.tab)
}
