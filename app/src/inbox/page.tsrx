import { useEffect, useMemo, useState } from "preact/hooks"
import type {
  BulkUpdateInboxItemsRequest,
  CompleteSessionInboxItemRequest,
  InboxEntityId,
  InboxItem,
  UpdateInboxItemRequest,
} from "@goddard-ai/inbox/schema"
import { useSearchNavigation } from "@goddard-ai/ui-primitives"

import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
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
  filterId: InboxFilterId;
  realtimeErrorMessage: string | null;
  searchQuery: string;
  selectedEntityIds: ReadonlySet<InboxEntityId>;
  showProjectNames: boolean;
  onBulkAction: (label: string, input: Omit<BulkUpdateInboxItemsRequest, "entityIds">) => void;
  onClearSelectedItems: () => void;
  onFilterSelect: (id: InboxFilterId) => void;
  onItemSelectionChange: (item: InboxItem, isSelected: boolean) => void;
  onOpenItem: (item: InboxItem) => void;
  onRefresh: () => void;
  onSearchInput: (value: string) => void;
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
  const visibleItemIds = visibleItems.map((item) => item.id).join("\n")

  const navigation = useSearchNavigation({
    activeAttribute: "data-active",
    count: () => visibleItems.length,
    onActivate(index) {
      const item = visibleItems[index]

      if (item) {
        props.onOpenItem(item)
      }
    },
    onQueryChange(query) {
      props.onSearchInput(query)
    },
  })

  useEffect(() => {
    handleInboxItemsLoaded(items)
  }, [items])

  useEffect(() => {
    navigation.resetActiveIndex()
  }, [navigation, visibleItemIds])

  <>
    <InboxPageControls
      filterId={props.filterId}
      inputRef={navigation.inputRef}
      searchQuery={props.searchQuery}
      selectedCount={props.selectedEntityIds.size}
      onBulkAction={props.onBulkAction}
      onClearSelectedItems={props.onClearSelectedItems}
      onFilterSelect={props.onFilterSelect}
      onRefresh={() => {
        navigation.resetActiveIndex()
        props.onRefresh()
      }}
      onSearchInput={props.onSearchInput}
    />
    <div class={styles.listBody}>
      @if (props.realtimeErrorMessage) {
        <div class={styles.realtimeNotice} role="status" title={props.realtimeErrorMessage}>
          Realtime inbox updates are unavailable. Manual refresh still works.
        </div>
      }

      <InboxList
        filterId={props.filterId}
        itemRef={navigation.itemRef}
        items={visibleItems}
        selectedEntityIds={props.selectedEntityIds}
        searchQuery={props.searchQuery}
        showProjectNames={props.showProjectNames}
        onItemSelectionChange={props.onItemSelectionChange}
      />
    </div>
  </>
}

function InboxPageControls(props: {
  filterId: InboxFilterId;
  inputRef?: (element: HTMLInputElement | null) => void | (() => void);
  searchQuery: string;
  selectedCount: number;
  onBulkAction: (label: string, input: Omit<BulkUpdateInboxItemsRequest, "entityIds">) => void;
  onClearSelectedItems: () => void;
  onFilterSelect: (id: InboxFilterId) => void;
  onRefresh: () => void;
  onSearchInput: (value: string) => void;
}) @{
  <>
    <InboxToolbar
      filterId={props.filterId}
      inputRef={props.inputRef}
      searchQuery={props.searchQuery}
      onFilterSelect={props.onFilterSelect}
      onRefresh={props.onRefresh}
      onSearchInput={props.inputRef ? undefined : props.onSearchInput}
    />
    <InboxBulkToolbar
      selectedCount={props.selectedCount}
      onArchive={() => {
        props.onBulkAction("Archive selected", { status: "archived" })
      }}
      onClear={props.onClearSelectedItems}
      onMarkRead={() => {
        props.onBulkAction("Mark selected read", { status: "read" })
      }}
      onMarkUnread={() => {
        props.onBulkAction("Mark selected unread", { status: "unread" })
      }}
      onSave={() => {
        props.onBulkAction("Save selected", { status: "saved" })
      }}
      onSetLowPriority={() => {
        props.onBulkAction("Set selected low priority", { priority: "low" })
      }}
      onSetNormalPriority={() => {
        props.onBulkAction("Set selected normal priority", { priority: "normal" })
      }}
    />
  </>
}

export default function InboxPage(props: { projectName?: string; projectPath?: string }) @{
  const workbenchTabSet = useWorkbenchTabSet()
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")
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

  function runBulkActionFromControls(
    label: string,
    input: Omit<BulkUpdateInboxItemsRequest, "entityIds">,
  ) {
    void runBulkInboxAction(label, input)
  }

  function refreshInboxList() {
    clearSelectedInboxItems()
    invalidateInboxQueries()
  }

  function updateSearchQuery(value: string) {
    clearSelectedInboxItems()
    setSearchQuery(value)
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
    <div class={styles.root}>
      @try {
        <InboxQueryContent
          {filterId}
          {realtimeErrorMessage}
          {searchQuery}
          {selectedEntityIds}
          showProjectNames={!props.projectPath}
          onBulkAction={runBulkActionFromControls}
          onClearSelectedItems={clearSelectedInboxItems}
          onFilterSelect={selectInboxFilter}
          onItemSelectionChange={setInboxItemSelected}
          onOpenItem={(item) => {
            void openInboxItem(item)
          }}
          onRefresh={refreshInboxList}
          onSearchInput={updateSearchQuery}
        />
      } @pending {
        <>
          <InboxPageControls
            {filterId}
            {searchQuery}
            selectedCount={selectedEntityIds.size}
            onBulkAction={runBulkActionFromControls}
            onClearSelectedItems={clearSelectedInboxItems}
            onFilterSelect={selectInboxFilter}
            onRefresh={refreshInboxList}
            onSearchInput={updateSearchQuery}
          />
          <div class={styles.listBody}>
            @if (realtimeErrorMessage) {
              <div class={styles.realtimeNotice} role="status" title={realtimeErrorMessage}>
                Realtime inbox updates are unavailable. Manual refresh still works.
              </div>
            }

            <InboxList
              {filterId}
              items={[]}
              listStatus="loading"
              {searchQuery}
              {selectedEntityIds}
              showProjectNames={!props.projectPath}
              onItemSelectionChange={setInboxItemSelected}
            />
          </div>
        </>
      } @catch (error, retry) {
        const resetInboxBoundary = () => {
          refreshInboxList()
          retry()
        }
        useErrorBoundaryReset(resetInboxBoundary)

        <>
          <InboxPageControls
            {filterId}
            {searchQuery}
            selectedCount={selectedEntityIds.size}
            onBulkAction={runBulkActionFromControls}
            onClearSelectedItems={clearSelectedInboxItems}
            onFilterSelect={selectInboxFilter}
            onRefresh={refreshInboxList}
            onSearchInput={updateSearchQuery}
          />
          <div class={styles.listBody}>
            @if (realtimeErrorMessage) {
              <div class={styles.realtimeNotice} role="status" title={realtimeErrorMessage}>
                Realtime inbox updates are unavailable. Manual refresh still works.
              </div>
            }

            <InboxList
              errorMessage={getErrorMessage(error)}
              {filterId}
              items={[]}
              listStatus="error"
              {searchQuery}
              {selectedEntityIds}
              showProjectNames={!props.projectPath}
              onItemSelectionChange={setInboxItemSelected}
            />
          </div>
        </>
      }
    </div>
  </InboxPageMutations>
}
