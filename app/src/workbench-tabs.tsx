import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { X } from "lucide-react"
import { SvgIcon } from "./svg-icon"
import type { WorkbenchTab } from "./workbench-tab-set"
import { getWorkbenchTabIcon } from "./workbench-tab-registry"

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
    <div
      class={css({
        display: "flex",
        flexDirection: "column",
        borderBottom: "1px solid",
        borderColor: "border",
        background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.surface")} 100%)`,
      })}
    >
      <div
        class={css({
          display: "flex",
          alignItems: "stretch",
          gap: "8px",
          padding: "14px 16px 0",
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
      class={css({
        display: "inline-flex",
        alignItems: "center",
        gap: "10px",
        minWidth: "0",
        maxWidth: "280px",
        height: "44px",
        paddingInline: "14px",
        border: "1px solid",
        borderColor: "border",
        borderTopLeftRadius: "16px",
        borderTopRightRadius: "16px",
        borderBottomWidth: "0",
        background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
        color: "muted",
        cursor: "pointer",
        transition:
          "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
        _active: {
          transform: "scale(0.985)",
        },
        "@media (hover: hover) and (pointer: fine)": {
          _hover: {
            color: "text",
            borderColor: "accent",
          },
        },
        "&[data-active='true']": {
          backgroundColor: "background",
          color: "text",
          borderColor: "accent",
          boxShadow: `0 14px 28px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent)`,
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
        <SvgIcon name={getWorkbenchTabIcon(props.tab.kind)} height="16px" width="16px" />
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
      {props.tab.kind !== "main" ? (
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
              width: "24px",
              height: "24px",
              borderRadius: "999px",
              border: "none",
              backgroundColor: "transparent",
              color: "muted",
              cursor: "pointer",
              transition:
                "background-color 140ms cubic-bezier(0.23, 1, 0.32, 1), color 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
              _active: {
                transform: "scale(0.94)",
              },
              "@media (hover: hover) and (pointer: fine)": {
                _hover: {
                  backgroundColor: "background",
                  color: "text",
                },
              },
            })}
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
