import { useState } from "preact/hooks"
import type { InboxItem } from "@goddard-ai/schema/daemon"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { goddardSdk } from "~/sdk.ts"
import { useQuery } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { getPullRequestDisplayTitle } from "~/pull-requests/presentation.ts"
import { getSessionDisplayTitle } from "~/sessions/presentation.ts"
import { InboxList } from "./list.tsrx"
import { InboxPageMutations, invalidateInboxQueries } from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import { getInboxListRequest } from "./queries.ts"
import {
  DEFAULT_INBOX_FILTER_ID,
  filterInboxItemsBySearch,
  isInboxEntityKind,
  type InboxFilterId,
} from "./presentation.ts"
import styles from "./page.style.ts"

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon-backed inbox request failed."
}

component InboxQueryContent(props: {
  filterId: InboxFilterId;
  searchQuery: string;
}) {
  const { items } = useQuery(goddardSdk.inbox.list, [getInboxListRequest(props.filterId)])
  const visibleItems = filterInboxItemsBySearch(items, props.searchQuery)

  <InboxList filterId={props.filterId} items={visibleItems} searchQuery={props.searchQuery} />
}

component InboxPage() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")

  async function openInboxItem(item: InboxItem) {
    try {
      if (isInboxEntityKind(item, "session")) {
        const { session } = await goddardSdk.session.get(
          {
            id: item.entityId,
          },
        )

        workbenchTabSet.openOrFocusTab(
          {
            id: `session:${session.id}`,
            kind: "sessionChat",
            title: getSessionDisplayTitle(session),
            payload: {
              projectPath: findNearestProjectPath(projectRegistry.projectList, session.cwd),
              sessionId: session.id,
            },
            dirty: false,
          },
        )
        return
      }

      if (!isInboxEntityKind(item, "pullRequest")) {
        return
      }

      const { pullRequest } = await goddardSdk.pr.get(
        {
          id: item.entityId,
        },
      )

      workbenchTabSet.openOrFocusTab(
        {
          id: `pull-request:${pullRequest.id}`,
          kind: "pullRequest",
          title: getPullRequestDisplayTitle(pullRequest),
          payload: {
            projectPath: findNearestProjectPath(projectRegistry.projectList, pullRequest.cwd),
            pullRequestId: pullRequest.id,
          },
          dirty: false,
        },
      )
    } catch (error) {
      console.error("Failed to open inbox item.", error)
      appToaster.create(
        {
          description: "The linked daemon entity could not be loaded.",
          duration: 3200,
          title: "Couldn't open inbox item",
          type: "error",
        },
      )
    }
  }

  <InboxPageMutations
    mutations={{
      openInboxItem,
    }}
  >
    <div class={styles.root}>
      <InboxToolbar
        {filterId}
        {searchQuery}
        onFilterSelect={(id) => {
          setFilterId(id)
        }}
        onRefresh={() => {
          invalidateInboxQueries()
        }}
        onSearchInput={(value) => {
          setSearchQuery(value)
        }}
      />
      <div class={styles.listBody}>
        try {
          <InboxQueryContent {filterId} {searchQuery} />
        } pending {
          <InboxList {filterId} items={[]} listStatus="loading" {searchQuery} />
        } catch (error) {
          <InboxList
            errorMessage={getErrorMessage(error)}
            {filterId}
            items={[]}
            listStatus="error"
            {searchQuery}
          />
        }
      </div>
    </div>
  </InboxPageMutations>
}

export default InboxPage
