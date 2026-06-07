import type { InboxItem } from "@goddard-ai/inbox/schema"

import { getPullRequestDisplayTitle } from "~/pull-requests/display.ts"
import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle } from "~/sessions/display.ts"
import { evictSessionHistory } from "~/sessions/mutations.ts"
import type { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import { isInboxEntityKind } from "./entity-kind.ts"

/** Opens the workbench tab linked to one daemon inbox row. */
export async function openInboxItemInWorkbench(input: {
  item: InboxItem
  workbenchTabSet: Pick<WorkbenchTabSet, "openOrFocusTab">
}) {
  const { item, workbenchTabSet } = input

  if (isInboxEntityKind(item, "session")) {
    const { session } = await goddardSdk.session.get({
      id: item.entityId,
    })

    evictSessionHistory(session.id)
    workbenchTabSet.openOrFocusTab({
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: session.cwd,
        sessionId: session.id,
        sessionTitle: getSessionDisplayTitle(session),
      },
    })
    return
  }

  if (!isInboxEntityKind(item, "pullRequest")) {
    return
  }

  const { pullRequest } = await goddardSdk.pr.get({
    id: item.entityId,
  })

  workbenchTabSet.openOrFocusTab({
    kind: "pullRequest",
    props: {
      relatedFilesystemPath: pullRequest.cwd,
      pullRequestId: pullRequest.id,
      pullRequestTitle: getPullRequestDisplayTitle(pullRequest),
    },
  })
}
