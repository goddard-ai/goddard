/** Presentational chrome primitives for the merged app shell layout. */
import { css, cx } from "@goddard-ai/styled-system/css"
import type { ComponentChildren } from "preact"
import { useState } from "preact/hooks"
import { ShellIcon } from "../support/shell-icons"
import { SvgIcon, type SvgIconName } from "../support/svg-icon"
import {
  formatBadgeCount,
  getTabIcon,
  type AppShellIconSource,
  type AppShellTopbarAction,
} from "./AppShell.config"
import type { NavigationItemId } from "./state/Navigation"
import type { WorkbenchTab } from "./state/WorkbenchTabRegistry"
import { WORKBENCH_PRIMARY_TAB } from "./state/WorkbenchTabSet"

const actionButtonClass = css({
  position: "relative",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "none",
  backgroundColor: "transparent",
  color: "#66a1ff",
  cursor: "pointer",
  transition:
    "background-color 200ms cubic-bezier(0.23, 1, 0.32, 1), color 200ms cubic-bezier(0.23, 1, 0.32, 1), opacity 200ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    outline: "2px solid #66a1ff",
    outlineOffset: "2px",
  },
})

/** Renders the merged shell chrome around the active workbench content. */
export function AppShellChrome(props: {
  activeTabId: string
  children?: ComponentChildren
  indicator: {
    left: number
    opacity: number
    width: number
  }
  navigationItems: Array<{
    ariaLabel: string
    badgeCount?: number
    group: "primary" | "secondary"
    icon: SvgIconName
    id: NavigationItemId
    label: string
  }>
  onAction: (action: AppShellTopbarAction) => void
  onNavigationSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
  onTabClose: (id: string) => void
  onTabDragEnd: () => void
  onTabDragEnter: (id: string) => void
  onTabDragStart: (id: string) => void
  onTabSelect: (id: string) => void
  selectedNavigationId: NavigationItemId
  selectedNavigationLabel: string
  setTabRef: (id: string, element: HTMLDivElement | null) => void
  tabStripRef: { current: HTMLDivElement | null }
  tabs: readonly Exclude<WorkbenchTab, WorkbenchTab<"main">>[]
}) {
  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "42px minmax(0, 1fr)",
        gridTemplateRows: "48px minmax(0, 1fr)",
        width: "100%",
        height: "100vh",
        maxHeight: "100vh",
        overflow: "hidden",
        backgroundColor: "#181818",
        color: "#ffffff",
        fontFamily: '"Inter Tight", sans-serif',
      })}
    >
      <header
        class={cx(
          css({
            gridColumn: "1 / -1",
            position: "relative",
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            height: "48px",
            paddingRight: "11px",
            backgroundColor: "#1c1c1c",
            boxShadow: "0 0.5px 0 #313131",
            zIndex: "2",
          }),
          "electrobun-webkit-app-region-drag",
        )}
      >
        {/* <SvgIcon
          class={css({
            position: "absolute",
            top: "10px",
            left: "19px",
            width: "137px",
            height: "28px",
            userSelect: "none",
          })}
          name="logo"
          height="28"
          title="Goddard"
          viewBox="0 0 137 28"
          width="137"
        /> */}
        <div
          class={css({
            position: "absolute",
            left: "50%",
            top: "10px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            width: "min(40%, 320px)",
            minWidth: "140px",
            height: "28px",
            paddingInline: "16px",
            borderRadius: "6px",
            backgroundColor: "#262626",
            transform: "translateX(-50%)",
          })}
        >
          <span
            class={css({
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              textAlign: "center",
              fontSize: "13px",
              fontWeight: "400",
              letterSpacing: "0.02em",
              lineHeight: "1.21",
              color: "rgba(255, 255, 255, 0.6)",
            })}
          >
            {props.selectedNavigationLabel}
          </span>
        </div>
        <div
          class={cx(
            css({
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-end",
              gap: "12px",
              height: "28px",
            }),
            "electrobun-webkit-app-region-no-drag",
          )}
        >
          <div
            class={css({
              display: "flex",
              alignItems: "center",
              padding: "3px",
              borderRadius: "14px",
              backgroundColor: "rgba(51, 130, 255, 0.18)",
              _hover: {
                "& > span": {
                  opacity: "0",
                },
              },
            })}
          >
            <ActionButton
              action="proposeTask"
              ariaLabel="Propose task"
              icon="what-next"
              onAction={props.onAction}
            />
            <span
              class={css({
                width: "1px",
                height: "10px",
                backgroundColor: "#66a1ff",
                opacity: "0.3",
                transition: "opacity 200ms cubic-bezier(0.23, 1, 0.32, 1)",
              })}
            />
            <ActionButton
              action="newSession"
              ariaLabel="New session"
              icon="new-session"
              onAction={props.onAction}
            />
          </div>
          <button
            aria-label="Settings"
            class={cx(
              actionButtonClass,
              css({
                width: "32px",
                height: "28px",
                borderRadius: "9px",
                color: "#999999",
                _hover: {
                  backgroundColor: "#262626",
                },
              }),
            )}
            type="button"
            onClick={() => {
              props.onAction("settings")
            }}
          >
            <InlineSvgIcon icon="settings" size="15px" />
          </button>
        </div>
      </header>
      <nav
        aria-label="Primary"
        class={css({
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "flex-start",
          width: "42px",
          height: "100%",
          overflowY: "auto",
          overflowX: "hidden",
          background: "linear-gradient(180deg, #1c1c1c 12.32%, #181818 100%)",
        })}
      >
        <AppShellSidebarSection
          items={props.navigationItems.filter((item) => item.group === "primary")}
          onNavigationSelect={props.onNavigationSelect}
          selectedNavigationId={props.selectedNavigationId}
        />
        <div
          aria-hidden="true"
          class={css({
            width: "70%",
            minHeight: "0.5px",
            marginTop: "6px",
            marginBottom: "6px",
            backgroundColor: "#2f2f2f",
          })}
        />
        <AppShellSidebarSection
          items={props.navigationItems.filter((item) => item.group === "secondary")}
          onNavigationSelect={props.onNavigationSelect}
          selectedNavigationId={props.selectedNavigationId}
        />
      </nav>
      <div
        class={css({
          display: "grid",
          gridTemplateRows: "31px minmax(0, 1fr)",
          minWidth: "0",
          minHeight: "0",
        })}
      >
        <div
          ref={props.tabStripRef}
          class={css({
            position: "relative",
            display: "flex",
            alignItems: "stretch",
            minHeight: "31px",
            height: "31px",
            overflowX: "auto",
            overflowY: "hidden",
            backgroundColor: "#1c1c1c",
            boxShadow: "inset 0 -0.5px 0 #313131",
          })}
        >
          <div
            class={css({
              position: "relative",
              display: "flex",
              alignItems: "stretch",
              minWidth: "max-content",
              height: "31px",
            })}
            role="tablist"
            aria-label="Workbench tabs"
          >
            <TabButton
              activeTabId={props.activeTabId}
              icon={getTabIcon("main")}
              id={WORKBENCH_PRIMARY_TAB.id}
              isHome={true}
              onSelect={props.onTabSelect}
              refCallback={(element) => {
                props.setTabRef(WORKBENCH_PRIMARY_TAB.id, element)
              }}
              title={props.selectedNavigationLabel}
            />
            {props.tabs.map((tab) => (
              <TabButton
                key={tab.id}
                activeTabId={props.activeTabId}
                dirty={tab.dirty}
                icon={getTabIcon(tab.icon)}
                id={tab.id}
                onClose={props.onTabClose}
                onDragEnd={props.onTabDragEnd}
                onDragEnter={props.onTabDragEnter}
                onDragStart={props.onTabDragStart}
                onSelect={props.onTabSelect}
                refCallback={(element) => {
                  props.setTabRef(tab.id, element)
                }}
                title={tab.title}
              />
            ))}
            <span
              aria-hidden="true"
              class={css({
                position: "absolute",
                bottom: "0",
                height: "0.5px",
                backgroundColor: "#cccccc",
                transition:
                  "transform 180ms cubic-bezier(0.23, 1, 0.32, 1), width 180ms cubic-bezier(0.23, 1, 0.32, 1), opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              })}
              style={{
                opacity: props.indicator.opacity,
                transform: `translateX(${props.indicator.left}px)`,
                width: `${props.indicator.width}px`,
              }}
            />
          </div>
        </div>
        <div
          class={css({
            minHeight: "0",
            overflow: "hidden",
          })}
        >
          {props.children}
        </div>
      </div>
    </div>
  )
}

/** Renders one grouped section of the left navigation rail. */
function AppShellSidebarSection(props: {
  items: Array<{
    ariaLabel: string
    badgeCount?: number
    icon: SvgIconName
    id: NavigationItemId
  }>
  onNavigationSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
  selectedNavigationId: NavigationItemId
}) {
  return (
    <>
      {props.items.map((item) => (
        <SidebarItem
          key={item.id}
          badgeCount={item.badgeCount}
          icon={item.icon}
          isSelected={item.id === props.selectedNavigationId}
          label={item.ariaLabel}
          onSelect={(options) => {
            props.onNavigationSelect(item.id, options)
          }}
        />
      ))}
    </>
  )
}

function ActionButton(props: {
  action: Exclude<AppShellTopbarAction, "settings">
  ariaLabel: string
  icon: SvgIconName
  onAction: (action: AppShellTopbarAction) => void
}) {
  return (
    <button
      aria-label={props.ariaLabel}
      class={cx(
        actionButtonClass,
        css({
          width: "46px",
          height: "22px",
          borderRadius: "11px",
          _hover: {
            backgroundColor: "#2a3f61",
          },
        }),
      )}
      type="button"
      onClick={() => {
        props.onAction(props.action)
      }}
    >
      <InlineSvgIcon
        icon={props.icon}
        size={props.action === "proposeTask" ? "17px 12px" : "16px 16px"}
      />
    </button>
  )
}

function SidebarItem(props: {
  badgeCount?: number
  icon: SvgIconName
  isSelected: boolean
  label: string
  onSelect: (options?: { openInTab?: boolean }) => void
}) {
  return (
    <button
      aria-current={props.isSelected ? "page" : undefined}
      aria-label={props.label}
      class={css({
        position: "relative",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: "42px",
        minWidth: "42px",
        height: "42px",
        minHeight: "42px",
        border: "none",
        backgroundColor: "transparent",
        color: "#5a5a5a",
        cursor: "pointer",
        transition: "color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
        _focusVisible: {
          outline: "2px solid #66a1ff",
          outlineOffset: "-2px",
        },
        _hover: {
          color: "#ffffff",
        },
      })}
      style={{
        color: props.isSelected ? "#ffffff" : "#5a5a5a",
      }}
      type="button"
      onClick={(event) => {
        props.onSelect({ openInTab: event.metaKey })
      }}
    >
      <InlineSvgIcon icon={props.icon} size="22px 22px" />
      {props.badgeCount ? (
        <span
          aria-hidden="true"
          class={css({
            position: "absolute",
            top: "4px",
            right: "3px",
            minWidth: "14px",
            height: "14px",
            paddingInline: "4px",
            borderRadius: "999px",
            backgroundColor: "#66a1ff",
            color: "#0f1115",
            fontSize: "9px",
            fontWeight: "700",
            lineHeight: "14px",
            textAlign: "center",
            boxShadow: "0 6px 16px rgba(102, 161, 255, 0.24)",
          })}
        >
          {formatBadgeCount(props.badgeCount)}
        </span>
      ) : null}
    </button>
  )
}

function TabButton(props: {
  activeTabId: string
  dirty?: boolean
  icon: AppShellIconSource
  id: string
  isHome?: boolean
  onClose?: (id: string) => void
  onDragEnd?: () => void
  onDragEnter?: (id: string) => void
  onDragStart?: (id: string) => void
  onSelect: (id: string) => void
  refCallback: (element: HTMLDivElement | null) => void
  title: string
}) {
  const isActive = props.activeTabId === props.id
  const isHome = props.isHome === true
  const [isHovered, setIsHovered] = useState(false)
  const iconColor = isActive
    ? isHome
      ? "#cccccc"
      : "#ffffff"
    : isHovered
      ? "#ffffff"
      : isHome
        ? "#7a7a7a"
        : "#6b6b6b"
  const labelColor = isActive
    ? isHome
      ? "#cccccc"
      : "#ffffff"
    : isHovered
      ? "#ffffff"
      : isHome
        ? "#7a7a7a"
        : "rgba(255, 255, 255, 0.35)"

  return (
    <div
      ref={props.refCallback}
      class={css({
        position: "relative",
        display: "inline-flex",
        alignItems: "stretch",
        height: "30px",
        flexShrink: "0",
      })}
      draggable={!isHome}
      role="presentation"
      onDragEnd={() => {
        props.onDragEnd?.()
      }}
      onDragEnter={() => {
        props.onDragEnter?.(props.id)
      }}
      onDragOver={(event) => {
        if (!isHome) {
          event.preventDefault()
        }
      }}
      onDragStart={() => {
        props.onDragStart?.(props.id)
      }}
      onMouseEnter={() => {
        setIsHovered(true)
      }}
      onMouseLeave={() => {
        setIsHovered(false)
      }}
    >
      <button
        aria-selected={isActive}
        class={css({
          position: "relative",
          display: "inline-flex",
          alignItems: "center",
          height: "30px",
          flexShrink: "0",
          border: "none",
          borderRadius: "0",
          backgroundColor: "#1c1c1c",
          cursor: "pointer",
          transition:
            "color 180ms cubic-bezier(0.23, 1, 0.32, 1), opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          _focusVisible: {
            outline: "2px solid #66a1ff",
            outlineOffset: "-2px",
          },
        })}
        style={{
          paddingLeft: "8px",
          paddingRight: isHome ? "14px" : "34px",
        }}
        role="tab"
        type="button"
        onClick={() => {
          props.onSelect(props.id)
        }}
      >
        <span
          class={css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: "20px",
            height: "20px",
            marginRight: "5px",
            marginTop: "5px",
            marginBottom: "5px",
            transition: "color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          })}
          style={{
            color: iconColor,
          }}
        >
          <AppShellResolvedIcon icon={props.icon} size="14px" />
        </span>
        <span
          class={css({
            fontSize: "12px",
            fontWeight: "400",
            letterSpacing: "0.02em",
            lineHeight: "1.21",
            whiteSpace: "nowrap",
            transition: "color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          })}
          style={{
            color: labelColor,
            marginRight: isHome ? "0" : "6px",
          }}
        >
          {props.title}
        </span>
      </button>
      {isHome ? null : (
        <>
          {props.dirty ? (
            <span
              aria-hidden="true"
              class={css({
                position: "absolute",
                top: "11px",
                right: "15px",
                width: "6px",
                height: "6px",
                borderRadius: "999px",
                backgroundColor: "#66a1ff",
                transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              })}
              style={{
                opacity: isActive || isHovered ? "0" : "1",
              }}
            />
          ) : null}
          <button
            aria-label={`Close ${props.title}`}
            class={css({
              position: "absolute",
              top: "6px",
              right: "10px",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "18px",
              height: "18px",
              padding: "0",
              border: "none",
              borderRadius: "999px",
              backgroundColor: "transparent",
              color: "#6b6b6b",
              cursor: "pointer",
              opacity: "0",
              transition:
                "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              _focusVisible: {
                outline: "2px solid #66a1ff",
                outlineOffset: "1px",
                opacity: "1",
              },
              _hover: {
                color: "#ffffff",
              },
            })}
            style={{
              opacity: isActive || isHovered ? "1" : "0",
            }}
            type="button"
            onClick={(event) => {
              event.stopPropagation()
              props.onClose?.(props.id)
            }}
          >
            <InlineSvgIcon icon="close-tab" size="9px" />
          </button>
        </>
      )}
    </div>
  )
}

/** Resolves one shell icon to either a named SVG sprite or the ShellIcon fallback set. */
export function AppShellResolvedIcon(props: { icon: AppShellIconSource; size: string }) {
  if (props.icon.svgName) {
    return <InlineSvgIcon icon={props.icon.svgName} size={props.size} />
  }

  // TODO: Replace this ShellIcon fallback once the exported shell asset set covers this surface.
  return (
    <span
      class={css({
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: "0",
      })}
      style={{
        width: props.size.split(" ")[0] ?? props.size,
        height: props.size.split(" ")[1] ?? props.size,
      }}
    >
      <ShellIcon name={props.icon.fallbackName ?? "main"} />
    </span>
  )
}

function InlineSvgIcon(props: { icon: SvgIconName; size: string }) {
  const [width, height] = props.size.includes(" ")
    ? props.size.split(" ")
    : [props.size, props.size]

  return (
    <SvgIcon
      class={css({
        display: "inline-block",
        flexShrink: "0",
      })}
      name={props.icon}
      height={height}
      width={width}
    />
  )
}
