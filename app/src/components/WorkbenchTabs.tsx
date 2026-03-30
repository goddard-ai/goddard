import { useSignal } from "@preact/signals"
import { css } from "../../styled-system/css"
import type { WorkbenchDetailTab, WorkbenchPrimaryTab } from "../state/workbench-tabs-state"
import { ShellIcon } from "../support/shell-icons"

/** Renders the sprint-1 primary tab plus any closable detail tabs. */
export function WorkbenchTabs(props: {
  primaryTab: WorkbenchPrimaryTab
  detailTabs: readonly WorkbenchDetailTab[]
  activeTabId: string
  detailTabLimit: number
  onSelect: (id: string) => void
  onClose: (id: string) => void
  onReorder: (fromId: string, toId: string) => void
}) {
  const dragSourceId = useSignal<string | null>(null)

  return (
    <div
      class={css({
        display: "flex",
        flexDirection: "column",
        borderBottom: "1px solid",
        borderColor: "border",
        backgroundColor: "panel",
      })}
    >
      <div
        class={css({
          display: "flex",
          alignItems: "stretch",
          gap: "8px",
          padding: "10px 14px 0",
          overflowX: "auto",
        })}
        role="tablist"
        aria-label="Workbench tabs"
      >
        <WorkbenchTab
          isActive={props.activeTabId === props.primaryTab.id}
          isPrimary={true}
          onSelect={props.onSelect}
          tab={props.primaryTab}
        />
        {props.detailTabs.map((tab) => (
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
      <div
        class={css({
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "10px 16px 12px",
          color: "muted",
          fontSize: "0.82rem",
        })}
      >
        <span>
          {props.detailTabs.length === 0
            ? "No detail tabs open yet."
            : `${props.detailTabs.length} detail tabs open`}
        </span>
        <span>{props.detailTabLimit} tab limit</span>
      </div>
    </div>
  )
}

/** Renders one workbench tab chip, including close and drag affordances for detail tabs. */
function WorkbenchTab(props: {
  tab: WorkbenchPrimaryTab | WorkbenchDetailTab
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
      class={css({
        display: "inline-flex",
        alignItems: "center",
        gap: "10px",
        minWidth: "0",
        maxWidth: "280px",
        height: "42px",
        paddingInline: "14px",
        border: "1px solid",
        borderColor: "border",
        borderTopLeftRadius: "14px",
        borderTopRightRadius: "14px",
        borderBottomWidth: "0",
        backgroundColor: "surface",
        color: "muted",
        cursor: "pointer",
        transition: "background-color 140ms ease, color 140ms ease, border-color 140ms ease",
        _hover: {
          color: "text",
        },
        "&[data-active='true']": {
          backgroundColor: "background",
          color: "text",
          borderColor: "accent",
        },
      })}
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
      <span class={css({ width: "16px", height: "16px", flexShrink: "0" })}>
        <ShellIcon name={props.tab.icon} />
      </span>
      <span
        class={css({
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontSize: "0.92rem",
          fontWeight: "600",
        })}
      >
        {props.tab.title}
      </span>
      {"dirty" in props.tab ? (
        <span
          class={css({
            display: "inline-flex",
            alignItems: "center",
            gap: "8px",
            marginLeft: "auto",
          })}
        >
          {props.tab.dirty ? (
            <span
              class={css({
                width: "8px",
                height: "8px",
                borderRadius: "999px",
                backgroundColor: "accentStrong",
                flexShrink: "0",
              })}
            />
          ) : null}
          <button
            aria-label={`Close ${props.tab.title}`}
            class={css({
              display: "grid",
              placeItems: "center",
              width: "22px",
              height: "22px",
              borderRadius: "999px",
              border: "none",
              backgroundColor: "transparent",
              color: "muted",
              cursor: "pointer",
              _hover: {
                backgroundColor: "surface",
                color: "text",
              },
            })}
            type="button"
            onClick={(event) => {
              event.stopPropagation()
              props.onClose?.(props.tab.id)
            }}
          >
            ×
          </button>
        </span>
      ) : null}
    </div>
  )
}
