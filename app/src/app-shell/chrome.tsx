/** Presentational chrome primitives for the merged app shell layout. */
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useState } from "preact/hooks"

import { AppCommand } from "~/commands/app-command.ts"
import { maximizeWindow } from "~/desktop-host.ts"
import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsx"
import type { NavigationItemId } from "~/navigation.ts"
import { getWorkbenchTabIcon, type WorkbenchTab } from "~/workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "~/workbench-tab-set.ts"

const accentColor = token.var("colors.accent")
const accentStrongColor = token.var("colors.accentStrong")
const backgroundColor = token.var("colors.background")
const borderColor = token.var("colors.border")
const panelColor = token.var("colors.panel")
const textColor = token.var("colors.text")

const actionButtonClass = css({
  position: "relative",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "none",
  backgroundColor: "transparent",
  color: "accentStrong",
  cursor: "pointer",
  transition:
    "background-color 200ms cubic-bezier(0.23, 1, 0.32, 1), color 200ms cubic-bezier(0.23, 1, 0.32, 1), opacity 200ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    outline: "2px solid",
    outlineColor: "accentStrong",
    outlineOffset: "2px",
  },
})

/** Renders the merged shell chrome around the active workbench content. */
export function AppShellChrome(props: {
  activeTabId: string
  children?: preact.ComponentChildren
  indicator: {
    left: number
    opacity: number
    width: number
  }
  navigationItems: Array<{
    group: "primary" | "secondary"
    icon: SvgIconName
    id: NavigationItemId
    label: string
  }>
  onCommandMenuOpen: () => void
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
        backgroundColor: "background",
        color: "text",
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
            backgroundColor: "surface",
            boxShadow: `0 0.5px 0 ${borderColor}`,
            zIndex: "2",
          }),
          "electrobun-webkit-app-region-drag",
        )}
        onDblClick={(event) => {
          if (
            (event.target as HTMLElement | null)?.closest(".electrobun-webkit-app-region-no-drag")
          ) {
            return
          }

          void maximizeWindow()
        }}
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
          class={cx(
            css({
              position: "absolute",
              left: "50%",
              top: "10px",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "min(40%, 320px)",
              minWidth: "140px",
              transform: "translateX(-50%)",
            }),
            "electrobun-webkit-app-region-no-drag",
          )}
        >
          <button
            aria-label="Open command menu"
            class={css({
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "28px",
              paddingInline: "16px",
              border: "none",
              borderRadius: "6px",
              backgroundColor: "panel",
              color: "muted",
              cursor: "pointer",
              transition:
                "background-color 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              _hover: {
                backgroundColor: "surface",
                color: "text",
              },
              _focusVisible: {
                outline: "2px solid",
                outlineColor: "accentStrong",
                outlineOffset: "2px",
              },
            })}
            type="button"
            onClick={props.onCommandMenuOpen}
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
              })}
            >
              {props.selectedNavigationLabel}
            </span>
          </button>
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
              backgroundColor: `color-mix(in srgb, ${accentColor} 18%, transparent)`,
              _hover: {
                "& > span": {
                  opacity: "0",
                },
              },
            })}
          >
            <ActionButton
              ariaLabel="Propose task"
              icon={<InlineSvgIcon icon="what-next" size="17px 12px" />}
              onAction={AppCommand.proposeTask}
            />
            <span
              class={css({
                width: "1px",
                height: "10px",
                backgroundColor: accentStrongColor,
                opacity: "0.3",
                transition: "opacity 200ms cubic-bezier(0.23, 1, 0.32, 1)",
              })}
            />
            <ActionButton
              ariaLabel="New session"
              icon={<InlineSvgIcon icon="new-session" size="16px 16px" />}
              onAction={AppCommand.newSession}
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
                color: "muted",
                _hover: {
                  backgroundColor: "panel",
                  color: "text",
                },
              }),
            )}
            type="button"
            onClick={() => {
              AppCommand.openSettings()
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
          background: `linear-gradient(180deg, ${panelColor} 12.32%, ${backgroundColor} 100%)`,
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
            backgroundColor: "border",
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
            backgroundColor: "surface",
            boxShadow: `inset 0 -0.5px 0 ${borderColor}`,
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
              icon={getWorkbenchTabIcon("main")}
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
                icon={getWorkbenchTabIcon(tab.kind)}
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
                backgroundColor: "accentStrong",
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
    icon: SvgIconName
    id: NavigationItemId
    label: string
  }>
  onNavigationSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
  selectedNavigationId: NavigationItemId
}) {
  return (
    <>
      {props.items.map((item) => (
        <SidebarItem
          key={item.id}
          icon={item.icon}
          isSelected={item.id === props.selectedNavigationId}
          label={item.label}
          onSelect={(options) => {
            props.onNavigationSelect(item.id, options)
          }}
        />
      ))}
    </>
  )
}

function ActionButton(props: {
  ariaLabel: string
  icon: preact.JSX.Element
  onAction: () => void
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
            backgroundColor: `color-mix(in srgb, ${accentColor} 24%, ${panelColor})`,
          },
        }),
      )}
      type="button"
      onClick={() => {
        props.onAction()
      }}
    >
      {props.icon}
    </button>
  )
}

function SidebarItem(props: {
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
        color: `color-mix(in srgb, ${textColor} 38%, transparent)`,
        cursor: "pointer",
        transition: "color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
        _focusVisible: {
          outline: "2px solid",
          outlineColor: "accentStrong",
          outlineOffset: "-2px",
        },
        _hover: {
          color: "text",
        },
      })}
      style={{
        color: props.isSelected ? textColor : `color-mix(in srgb, ${textColor} 38%, transparent)`,
      }}
      type="button"
      onClick={(event) => {
        props.onSelect({ openInTab: event.metaKey })
      }}
    >
      <InlineSvgIcon icon={props.icon} size="22px 21px" />
    </button>
  )
}

function TabButton(props: {
  activeTabId: string
  dirty?: boolean
  icon: SvgIconName
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
  const showCloseButton = isActive || isHovered
  const iconColor = isActive
    ? isHome
      ? accentStrongColor
      : textColor
    : isHovered
      ? textColor
      : isHome
        ? `color-mix(in srgb, ${textColor} 52%, transparent)`
        : `color-mix(in srgb, ${textColor} 42%, transparent)`
  const labelColor = isActive
    ? isHome
      ? accentStrongColor
      : textColor
    : isHovered
      ? textColor
      : isHome
        ? `color-mix(in srgb, ${textColor} 52%, transparent)`
        : `color-mix(in srgb, ${textColor} 34%, transparent)`

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
          backgroundColor: "surface",
          cursor: "pointer",
          transition:
            "color 180ms cubic-bezier(0.23, 1, 0.32, 1), opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          _focusVisible: {
            outline: "2px solid",
            outlineColor: "accentStrong",
            outlineOffset: "-2px",
          },
        })}
        style={{
          paddingLeft: "8px",
          paddingRight: isHome ? "14px" : "8px",
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
          <InlineSvgIcon icon={props.icon} size="14px" />
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
          }}
        >
          {props.title}
        </span>
      </button>
      {isHome ? null : (
        <div
          class={css({
            display: "grid",
            placeItems: "center",
            width: "32px",
            height: "30px",
            marginRight: "0",
            flexShrink: "0",
            cursor: "pointer",
          })}
          onClick={() => {
            props.onSelect(props.id)
          }}
        >
          {props.dirty ? (
            <span
              aria-hidden="true"
              class={css({
                gridArea: "1 / 1",
                width: "6px",
                height: "6px",
                borderRadius: "999px",
                backgroundColor: "accentStrong",
                transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              })}
              style={{
                opacity: showCloseButton ? "0" : "1",
              }}
            />
          ) : null}
          <button
            aria-label={`Close ${props.title}`}
            class={css({
              gridArea: "1 / 1",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "18px",
              height: "18px",
              padding: "0",
              border: "none",
              borderRadius: "999px",
              backgroundColor: "transparent",
              color: `color-mix(in srgb, ${textColor} 42%, transparent)`,
              cursor: "pointer",
              opacity: "0",
              transition:
                "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), background-color 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              _focusVisible: {
                outline: "2px solid",
                outlineColor: "accentStrong",
                outlineOffset: "1px",
                opacity: "1",
                backgroundColor: `color-mix(in srgb, ${textColor} 10%, transparent)`,
              },
              _hover: {
                backgroundColor: `color-mix(in srgb, ${textColor} 10%, transparent)`,
                color: "text",
              },
            })}
            style={{
              opacity: showCloseButton ? "1" : "0",
              pointerEvents: showCloseButton ? "auto" : "none",
            }}
            type="button"
            onClick={(event) => {
              event.stopPropagation()
              props.onClose?.(props.id)
            }}
          >
            <InlineSvgIcon icon="close-tab" size="9px" />
          </button>
        </div>
      )}
    </div>
  )
}

function InlineSvgIcon(props: { icon: SvgIconName; size: string }) {
  const [width, height] = props.size.includes(" ")
    ? props.size.split(" ")
    : [props.size, props.size]

  return (
    <GoodIcon
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
