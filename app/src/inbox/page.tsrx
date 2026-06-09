import { useEffect, useMemo, useRef, useState } from "preact/hooks"
import type {
  BulkUpdateInboxItemsRequest,
  CompleteSessionInboxItemRequest,
  InboxEntityId,
  InboxItem,
  UpdateInboxItemRequest,
} from "@goddard-ai/inbox/schema"

import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { getNextKeyboardRowIndex } from "~/lib/row-keyboard-navigation.ts"
import { goddardSdk } from "~/sdk.ts"
import { useQuery } from "~/lib/query.ts"
import { InboxBulkToolbar } from "./bulk-toolbar.tsrx"
import {
  allInboxStatuses,
  DEFAULT_INBOX_FILTER_ID,
  inboxFilterDefinitions,
  type InboxFilterId,
} from "./filters.ts"
import { InboxList } from "./list.tsrx"
import {
  bulkUpdateInboxItems as runBulkUpdateInboxItems,
  completeSessionInboxItem as runCompleteSessionInboxItem,
  InboxPageMutations,
  invalidateInboxQueries,
  updateInboxItem as runUpdateInboxItem,
} from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import { getInboxListRequest } from "./queries.ts"
import {
  filterPreparedInboxItemsBySearch,
  parseInboxSearchQuery,
  prepareInboxSearchItems,
  replaceInboxSearchStatusFilters,
} from "./search.ts"
import { openInboxItemInWorkbench } from "./open.ts"
import { handleInboxItemsLoaded } from "./session-visit.ts"
import styles from "./page.style.ts"

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon-backed inbox request failed."
}

function InboxQueryContent(props: {
  activeItemId: InboxItem["id"] | null;
  filterId: InboxFilterId;
  searchQuery: string;
  selectedEntityIds: ReadonlySet<InboxEntityId>;
  onItemSelectionChange: (item: InboxItem, isSelected: boolean) => void;
}) @{
  const parsedSearch = parseInboxSearchQuery(props.searchQuery)
  const { items } = useQuery(goddardSdk.inbox.list, {
    params: [
      getInboxListRequest({
        filterId: props.filterId,
        statuses: parsedSearch.isActive ? parsedSearch.statuses ?? allInboxStatuses : undefined,
      }),
    ],
  })
  const preparedItems = useMemo(() => prepareInboxSearchItems(items), [items])
  const visibleItems = filterPreparedInboxItemsBySearch(preparedItems, parsedSearch)
  const resolvedActiveItemId = visibleItems.some((item) => item.id === props.activeItemId)
    ? props.activeItemId
    : visibleItems[0]?.id ?? null

  useEffect(() => {
    handleInboxItemsLoaded(items)
  }, [items])

  <InboxList
    filterId={props.filterId}
    activeItemId={resolvedActiveItemId}
    items={visibleItems}
    selectedEntityIds={props.selectedEntityIds}
    searchQuery={props.searchQuery}
    onItemSelectionChange={props.onItemSelectionChange}
  />
}

function InboxPage() @{
  const workbenchTabSet = useWorkbenchTabSet()
  const pageRef = useRef<HTMLDivElement | null>(null)
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")
  const [activeItemId, setActiveItemId] = useState<InboxItem["id"] | null>(null)
  const [realtimeErrorMessage, setRealtimeErrorMessage] = useState<string | null>(null)
  const [selectedEntityIds, setSelectedEntityIds] = useState<ReadonlySet<InboxEntityId>>(
    () => new Set(),
  )

  function selectInboxFilter(id: InboxFilterId) {
    clearSelectedInboxItems()
    if (searchQuery.trim().length > 0) {
      setSearchQuery(
        replaceInboxSearchStatusFilters(searchQuery, inboxFilterDefinitions[id].statuses),
      )
    } else {
      setFilterId(id)
    }
  }

  useAppCommand(AppCommand.inbox.selectUnreadFilter, () => {
    selectInboxFilter("unread")
  })
  useAppCommand(AppCommand.inbox.selectSavedFilter, () => {
    selectInboxFilter("saved")
  })
  useAppCommand(AppCommand.inbox.selectRepliedFilter, () => {
    selectInboxFilter("replied")
  })
  useAppCommand(AppCommand.inbox.selectCompletedFilter, () => {
    selectInboxFilter("completed")
  })
  useAppCommand(AppCommand.inbox.selectArchivedFilter, () => {
    selectInboxFilter("archived")
  })

  useEffect(() => {
    const controller = new AbortController()

    void (async () => {
      try {
        const items = await goddardSdk.inbox.streamItems(undefined, { signal: controller.signal })
        const iterator = items[Symbol.asyncIterator]()
        while (!controller.signal.aborted) {
          const result = await iterator.next()
          if (result.done) {
            break
          }

          const item = result.value
          setRealtimeErrorMessage(null)
          handleInboxItemsLoaded([item])
          invalidateInboxQueries()
        }
      } catch (error) {
        if (!controller.signal.aborted) {
          console.error("Failed to subscribe to inbox updates.", error)
          setRealtimeErrorMessage(getErrorMessage(error))
        }
      }
    })()

    return () => {
      controller.abort()
    }
  }, [])

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

  function getVisibleInboxRowElements() {
    return [...(pageRef.current?.querySelectorAll<HTMLElement>("[data-inbox-row-id]") ?? [])]
  }

  function getInboxRowElementItemId(element: HTMLElement | undefined) {
    return element?.dataset.inboxRowId as InboxItem["id"] | undefined ?? null
  }

  function getResolvedActiveItemId(rowElements: readonly HTMLElement[]) {
    if (rowElements.some((element) => element.dataset.inboxRowId === activeItemId)) {
      return activeItemId
    }

    return getInboxRowElementItemId(rowElements[0])
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
      const result = await bulkUpdateInboxItems({
        entityIds,
        ...input,
      })
      clearSelectedInboxItems()
      if (result.missingEntityIds.length > 0) {
        appToaster.create({
          description: `${result.missingEntityIds.length} selected rows could not be found.`,
          duration: 3600,
          title: "Some inbox items were missing",
          type: "error",
        })
      }
    } catch (error) {
      console.error(`Failed to run bulk inbox action: ${label}`, error)
      appToaster.create({
        description: "The selected inbox rows could not be updated.",
        duration: 3200,
        title: `Couldn't ${label.toLowerCase()}`,
        type: "error",
      })
    }
  }

  async function updateInboxItem(input: UpdateInboxItemRequest) {
    return await runUpdateInboxItem(input)
  }

  async function bulkUpdateInboxItems(input: BulkUpdateInboxItemsRequest) {
    return await runBulkUpdateInboxItems(input)
  }

  async function completeSessionInboxItem(input: CompleteSessionInboxItemRequest) {
    return await runCompleteSessionInboxItem(input)
  }

  async function openInboxItem(item: InboxItem) {
    try {
      await openInboxItemInWorkbench({ item, workbenchTabSet })
    } catch (error) {
      console.error("Failed to open inbox item.", error)
      appToaster.create({
        description: "The linked daemon entity could not be loaded.",
        duration: 3200,
        title: "Couldn't open inbox item",
        type: "error",
      })
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
    <div ref={pageRef} class={styles.root}>
      <InboxToolbar
        {filterId}
        {searchQuery}
        onFilterSelect={(id) => {
          selectInboxFilter(id)
        }}
        onRefresh={() => {
          clearSelectedInboxItems()
          setActiveItemId(null)
          invalidateInboxQueries()
        }}
        onRowNavigationKeyDown={(event) => {
          const rowElements = getVisibleInboxRowElements()
          const resolvedActiveItemId = getResolvedActiveItemId(rowElements)
          const activeItemIndex = rowElements.findIndex(
            (element) => element.dataset.inboxRowId === resolvedActiveItemId,
          )

          switch (event.key) {
            case "ArrowDown":
            case "ArrowUp": {
              event.preventDefault()
              const nextIndex = getNextKeyboardRowIndex({
                currentIndex: activeItemIndex,
                direction: event.key === "ArrowDown" ? 1 : -1,
                rowCount: rowElements.length,
              })
              setActiveItemId(
                nextIndex === null ? null : getInboxRowElementItemId(rowElements[nextIndex]),
              )
              return
            }
            case "Enter":
              if (resolvedActiveItemId) {
                event.preventDefault()
                rowElements[activeItemIndex]?.click()
              }
          }
        }}
        onSearchInput={(value) => {
          clearSelectedInboxItems()
          setActiveItemId(null)
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
        @if (realtimeErrorMessage) {
          <div class={styles.realtimeNotice} role="status" title={realtimeErrorMessage}>
            Realtime inbox updates are unavailable. Manual refresh still works.
          </div>
        }
        @try {
          <InboxQueryContent
            {activeItemId}
            {filterId}
            {searchQuery}
            {selectedEntityIds}
            onItemSelectionChange={setInboxItemSelected}
          />
        } @pending {
          <InboxList
            {filterId}
            {activeItemId}
            items={[]}
            listStatus="loading"
            {searchQuery}
            {selectedEntityIds}
            onItemSelectionChange={setInboxItemSelected}
          />
        } @catch (error) {
          <InboxList
            errorMessage={getErrorMessage(error)}
            {filterId}
            {activeItemId}
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
