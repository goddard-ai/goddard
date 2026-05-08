import { useMemo, useState } from "preact/hooks"
import type {
  BulkUpdateInboxItemsRequest,
  InboxEntityId,
  InboxItem,
} from "@goddard-ai/schema/daemon"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { goddardSdk } from "~/sdk.ts"
import { useQuery } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { getPullRequestDisplayTitle } from "~/pull-requests/display.ts"
import { getSessionDisplayTitle } from "~/sessions/display.ts"
import { isInboxEntityKind } from "./entity-kind.ts"
import { InboxBulkToolbar } from "./bulk-toolbar.tsrx"
import {
  allInboxStatuses,
  DEFAULT_INBOX_FILTER_ID,
  inboxFilterDefinitions,
  type InboxFilterId,
} from "./filters.ts"
import { InboxList } from "./list.tsrx"
import {
  bulkUpdateInboxItems,
  completeSessionInboxItem,
  InboxPageMutations,
  invalidateInboxQueries,
  updateInboxItem,
} from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import { getInboxListRequest } from "./queries.ts"
import {
  filterPreparedInboxItemsBySearch,
  parseInboxSearchQuery,
  prepareInboxSearchItems,
  replaceInboxSearchStatusFilters,
} from "./search.ts"
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
  selectedEntityIds: ReadonlySet<InboxEntityId>;
  onItemSelectionChange: (item: InboxItem, isSelected: boolean) => void;
}) {
  const parsedSearch = parseInboxSearchQuery(props.searchQuery)
  const { items } = useQuery(goddardSdk.inbox.list, [
    getInboxListRequest(
      {
        filterId: props.filterId,
        statuses: parsedSearch.isActive ? parsedSearch.statuses ?? allInboxStatuses : undefined,
      },
    ),
  ])
  const preparedItems = useMemo(() => prepareInboxSearchItems(items), [items])
  const visibleItems = filterPreparedInboxItemsBySearch(preparedItems, parsedSearch)

  <InboxList
    filterId={props.filterId}
    items={visibleItems}
    selectedEntityIds={props.selectedEntityIds}
    searchQuery={props.searchQuery}
    onItemSelectionChange={props.onItemSelectionChange}
  />
}

component InboxPage() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")
  const [selectedEntityIds, setSelectedEntityIds] = useState<ReadonlySet<InboxEntityId>>(
    () => new Set(),
  )

  function clearSelectedInboxItems() {
    setSelectedEntityIds(new Set())
  }

  function setInboxItemSelected(item: InboxItem, isSelected: boolean) {
    setSelectedEntityIds((currentIds) => {
      const nextIds = new Set(currentIds)
      if (isSelected) {
        nextIds.add(item.entityId)
      } else {
        nextIds.delete(item.entityId)
      }
      return nextIds
    })
  }

  async function runBulkInboxAction(
    label: string,
    input: Omit<BulkUpdateInboxItemsRequest, "entityIds">,
  ) {
    const entityIds = [...selectedEntityIds]
    if (entityIds.length === 0) {
      return
    }

    try {
      const result = await bulkUpdateInboxItems(
        {
          entityIds,
          ...input,
        },
      )
      clearSelectedInboxItems()
      if (result.missingEntityIds.length > 0) {
        appToaster.create(
          {
            description: `${result.missingEntityIds.length} selected rows could not be found.`,
            duration: 3600,
            title: "Some inbox items were missing",
            type: "error",
          },
        )
      }
    } catch (error) {
      console.error(`Failed to run bulk inbox action: ${label}`, error)
      appToaster.create(
        {
          description: "The selected inbox rows could not be updated.",
          duration: 3200,
          title: `Couldn't ${label.toLowerCase()}`,
          type: "error",
        },
      )
    }
  }

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
      bulkUpdateInboxItems,
      completeSessionInboxItem,
      openInboxItem,
      updateInboxItem,
    }}
  >
    <div class={styles.root}>
      <InboxToolbar
        {filterId}
        {searchQuery}
        onFilterSelect={(id) => {
          clearSelectedInboxItems()
          if (searchQuery.trim().length > 0) {
            setSearchQuery(
              replaceInboxSearchStatusFilters(searchQuery, inboxFilterDefinitions[id].statuses),
            )
          } else {
            setFilterId(id)
          }
        }}
        onRefresh={() => {
          clearSelectedInboxItems()
          invalidateInboxQueries()
        }}
        onSearchInput={(value) => {
          clearSelectedInboxItems()
          setSearchQuery(value)
        }}
      />
      <InboxBulkToolbar
        selectedCount={selectedEntityIds.size}
        onArchive={() => {
          void runBulkInboxAction("Archive selected", { status: "archived" })
        }}
        onClear={() => {
          clearSelectedInboxItems()
        }}
        onMarkRead={() => {
          void runBulkInboxAction("Mark selected read", { status: "read" })
        }}
        onMarkUnread={() => {
          void runBulkInboxAction("Mark selected unread", { status: "unread" })
        }}
        onSave={() => {
          void runBulkInboxAction("Save selected", { status: "saved" })
        }}
        onSetLowPriority={() => {
          void runBulkInboxAction("Set selected low priority", { priority: "low" })
        }}
        onSetNormalPriority={() => {
          void runBulkInboxAction("Set selected normal priority", { priority: "normal" })
        }}
      />
      <div class={styles.listBody}>
        try {
          <InboxQueryContent
            {filterId}
            {searchQuery}
            {selectedEntityIds}
            onItemSelectionChange={setInboxItemSelected}
          />
        } pending {
          <InboxList
            {filterId}
            items={[]}
            listStatus="loading"
            {searchQuery}
            {selectedEntityIds}
            onItemSelectionChange={setInboxItemSelected}
          />
        } catch (error) {
          <InboxList
            errorMessage={getErrorMessage(error)}
            {filterId}
            items={[]}
            listStatus="error"
            {searchQuery}
            {selectedEntityIds}
            onItemSelectionChange={setInboxItemSelected}
          />
        }
      </div>
    </div>
  </InboxPageMutations>
}

export default InboxPage
