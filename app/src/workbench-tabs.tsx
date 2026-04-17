import { useSignal } from "@preact/signals"
import { X } from "lucide-react"

import { GoodIcon } from "./lib/good-icon.tsx"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"
import type { WorkbenchTab } from "./workbench-tab-set.ts"
import styles from "./workbench-tabs.style.ts"

/** Renders the primary tab plus any closable workbench tabs. */
export function WorkbenchTabs(props: {
  primaryTab: WorkbenchTab<"main">
  tabs: readonly Exclude<WorkbenchTab, WorkbenchTab<"main">>[]
  activeTabId: string
  onSelect: (id: string) => void
  onClose: (id: string) => void
  onReorder: (fromId: string, toId: string) => void
}) {
  const dragSourceId = useSignal<string | null>(null)

  return (
    <div class={styles.root}>
      <div class={styles.list} role="tablist" aria-label="Workbench tabs">
        <WorkbenchTab
          isActive={props.activeTabId === props.primaryTab.id}
          isPrimary={true}
          onSelect={props.onSelect}
          tab={props.primaryTab}
        />
        {props.tabs.map((tab) => (
          <WorkbenchTab
            key={tab.id}
            isActive={props.activeTabId === tab.id}
            isPrimary={false}
            onClose={props.onClose}
            onDragEnd={() => {
              dragSourceId.value = null
            }}
            onDragEnter={(tabId) => {
              if (dragSourceId.value && dragSourceId.value !== tabId) {
                props.onReorder(dragSourceId.value, tabId)
              }
            }}
            onDragStart={(tabId) => {
              dragSourceId.value = tabId
            }}
            onSelect={props.onSelect}
            tab={tab}
          />
        ))}
      </div>
    </div>
  )
}

/** Renders one workbench tab chip, including close and drag affordances for closable tabs. */
function WorkbenchTab(props: {
  tab: WorkbenchTab
  isActive: boolean
  isPrimary: boolean
  onSelect: (id: string) => void
  onClose?: (id: string) => void
  onDragStart?: (id: string) => void
  onDragEnter?: (id: string) => void
  onDragEnd?: () => void
}) {
  return (
    <div
      class={styles.tab}
      data-active={props.isActive}
      draggable={!props.isPrimary}
      role="tab"
      aria-selected={props.isActive}
      onClick={() => {
        props.onSelect(props.tab.id)
      }}
      onDragEnd={() => {
        props.onDragEnd?.()
      }}
      onDragEnter={() => {
        props.onDragEnter?.(props.tab.id)
      }}
      onDragOver={(event) => {
        event.preventDefault()
      }}
      onDragStart={() => {
        props.onDragStart?.(props.tab.id)
      }}
    >
      <span class={styles.icon}>
        <GoodIcon name={getWorkbenchTabIcon(props.tab.kind)} height="16px" width="16px" />
      </span>
      <span class={styles.label}>{props.tab.title}</span>
      {props.tab.kind !== "main" ? (
        <span class={styles.actions}>
          {props.tab.dirty ? <span class={styles.dirty} /> : null}
          <button
            aria-label={`Close ${props.tab.title}`}
            class={styles.close}
            type="button"
            onClick={(event) => {
              event.stopPropagation()
              props.onClose?.(props.tab.id)
            }}
          >
            <X size={14} strokeWidth={2.1} />
          </button>
        </span>
      ) : null}
    </div>
  )
}
