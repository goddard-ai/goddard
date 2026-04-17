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
import styles from "./chrome.style.ts"

const accentStrongColor = token.var("colors.accentStrong")
const textColor = token.var("colors.text")

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
  onNavigationSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
  onTabClose: (id: string) => void
  onTabDragEnd: () => void
  onTabDragEnter: (id: string) => void
  onTabDragStart: (id: string) => void
  onTabSelect: (id: string) => void
  projectSwitcher?: preact.ComponentChildren
  selectedNavigationId: NavigationItemId
  selectedNavigationLabel: string
  setTabRef: (id: string, element: HTMLDivElement | null) => void
  tabStripRef: { current: HTMLDivElement | null }
  tabs: readonly Exclude<WorkbenchTab, WorkbenchTab<"main">>[]
}) {
  return (
    <div class={styles.root}>
      <header
        class={cx(styles.header, "electrobun-webkit-app-region-drag")}
        onDblClick={(event) => {
          if (
            (event.target as HTMLElement | null)?.closest(".electrobun-webkit-app-region-no-drag")
          ) {
            return
          }

          void maximizeWindow()
        }}
      >
        <div class={cx(styles.switcher, "electrobun-webkit-app-region-no-drag")}>
          <div
            class={css({
              position: "relative",
              width: "100%",
            })}
          >
            {props.projectSwitcher}
          </div>
        </div>
        <div class={cx(styles.actions, "electrobun-webkit-app-region-no-drag")}>
          <div class={styles.actionGroup}>
            <ActionButton
              ariaLabel="Propose task"
              icon={<InlineSvgIcon icon="what-next" size="17px 12px" />}
              onAction={AppCommand.navigation.openProposeTaskDialog}
            />
            <span class={styles.actionDivider} />
            <ActionButton
              ariaLabel="New session"
              icon={<InlineSvgIcon icon="new-session" size="16px 16px" />}
              onAction={AppCommand.navigation.openNewSessionDialog}
            />
          </div>
          <button
            aria-label="Settings"
            class={cx(styles.actionButton, styles.settingsButton)}
            type="button"
            onClick={() => {
              AppCommand.navigation.openSettings()
            }}
          >
            <InlineSvgIcon icon="settings" size="15px" />
          </button>
        </div>
      </header>
      <nav aria-label="Primary" class={styles.nav}>
        <AppShellSidebarSection
          items={props.navigationItems.filter((item) => item.group === "primary")}
          onNavigationSelect={props.onNavigationSelect}
          selectedNavigationId={props.selectedNavigationId}
        />
        <div aria-hidden="true" class={styles.navDivider} />
        <AppShellSidebarSection
          items={props.navigationItems.filter((item) => item.group === "secondary")}
          onNavigationSelect={props.onNavigationSelect}
          selectedNavigationId={props.selectedNavigationId}
        />
      </nav>
      <div class={styles.main}>
        <div ref={props.tabStripRef} class={styles.tabStrip}>
          <div class={styles.tabList} role="tablist" aria-label="Workbench tabs">
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
              class={styles.indicator}
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
      class={cx(styles.actionButton, styles.actionItem)}
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
      class={styles.sidebarItem}
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
      class={styles.tab}
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
        class={styles.tabButton}
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
          class={styles.tabIcon}
          style={{
            color: iconColor,
          }}
        >
          <InlineSvgIcon icon={props.icon} size="14px" />
        </span>
        <span
          class={styles.tabLabel}
          style={{
            color: labelColor,
          }}
        >
          {props.title}
        </span>
      </button>
      {isHome ? null : (
        <div
          class={styles.tabTail}
          onClick={() => {
            props.onSelect(props.id)
          }}
        >
          {props.dirty ? (
            <span
              aria-hidden="true"
              class={styles.tabDirty}
              style={{
                opacity: showCloseButton ? "0" : "1",
              }}
            />
          ) : null}
          <button
            aria-label={`Close ${props.title}`}
            class={styles.tabClose}
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

  return <GoodIcon class={styles.icon} name={props.icon} height={height} width={width} />
}
