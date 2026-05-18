import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { maximizeWindow } from "~/desktop-host.ts"
import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsrx"
import { closeTopmostModal } from "~/lib/modal-stack.ts"
import type { MainTabItem, MainTabItemId } from "~/main-tab-items.ts"
import { SwitchProjectDropdown } from "~/projects/switch-project-dropdown.tsrx"
import { getWorkbenchTabIcon } from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB } from "~/workbench-tab-set.ts"
import styles from "./chrome.style.ts"
import {
  appShellDefaultSidebarDot,
  appShellSidebarDots,
  type AppShellSidebarDot,
} from "./config.ts"
import { AppShellWorkbenchContent } from "./views.tsrx"

const accentStrongColor = token.var("colors.accentStrong")
const textColor = token.var("colors.text")

function useAppShellTabStrip(
  activeTabId: string,
  selectedMainTabId: string,
  tabs: readonly { id: string }[],
) {
  const tabStripRef = useRef<HTMLDivElement | null>(null)
  const tabRefs = useRef<Record<string, HTMLDivElement | null>>({})
  const dragSourceTabId = useRef<string | null>(null)
  const [indicator, setIndicator] = useState({ left: 0, width: 0, opacity: 0 })

  function syncIndicator() {
    const activeTabElement = tabRefs.current[activeTabId]
    const tabStripElement = tabStripRef.current

    if (!activeTabElement || !tabStripElement) {
      setIndicator(
        (current) => current.opacity === 0
          ? current
          : { left: current.left, width: current.width, opacity: 0 },
      )
      return
    }

    setIndicator({
      left: activeTabElement.offsetLeft,
      width: activeTabElement.offsetWidth,
      opacity: 1,
    })
  }

  useLayoutEffect(() => {
    syncIndicator()
  }, [activeTabId, selectedMainTabId, tabs])

  useEffect(() => {
    const tabStripElement = tabStripRef.current

    if (!tabStripElement) {
      return
    }

    const observer = new ResizeObserver(() => {
      syncIndicator()
    })

    observer.observe(tabStripElement)

    for (const tabElement of Object.values(tabRefs.current)) {
      if (tabElement) {
        observer.observe(tabElement)
      }
    }

    return () => {
      observer.disconnect()
    }
  }, [selectedMainTabId, tabs])

  return {
    dragSourceTabId,
    indicator,
    tabRefs,
    tabStripRef,
  }
}

/** Renders the merged shell chrome and owns its local interaction wiring. */
export component AppShellChrome() {
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()
  const mainTabItems = mainTab.items.map(
    (item) => ({
      ...item,
      icon: getWorkbenchTabIcon(item.id),
    }),
  )
  const selectedMainTab =
    mainTabItems.find((item) => item.id === mainTab.selectedKind) ?? mainTabItems[0]
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    mainTab.selectedKind,
    workbenchTabSet.tabList,
  )

  function navigateMainTab(id: MainTabItemId) {
    mainTab.selectKind(id)
    workbenchTabSet.activateTab(WORKBENCH_MAIN_TAB.id)
  }

  useAppCommand(AppCommand.workbench.closeActiveTab, () => {
    if (closeTopmostModal()) {
      return
    }

    if (workbenchTabSet.activeTabId !== WORKBENCH_MAIN_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useAppCommand(AppCommand.navigation.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab({
      kind: "keyboardShortcuts",
      payload: {},
    })
  })

  useAppCommand(AppCommand.navigation.openInbox, () => {
    navigateMainTab("inbox")
  })

  useAppCommand(AppCommand.navigation.openSessions, () => {
    navigateMainTab("sessions")
  })

  useAppCommand(AppCommand.navigation.openSearch, () => {
    navigateMainTab("search")
  })

  useAppCommand(AppCommand.navigation.openSpecs, () => {
    navigateMainTab("specs")
  })

  useAppCommand(AppCommand.navigation.openTasks, () => {
    navigateMainTab("tasks")
  })

  useAppCommand(AppCommand.navigation.openRoadmap, () => {
    navigateMainTab("roadmap")
  })

  useAppCommand(AppCommand.navigation.openSettings, () => {
    workbenchTabSet.openOrFocusTab({
      kind: "settings",
      payload: {},
    })
  })

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
          try {
            <SwitchProjectDropdown />
          } catch (error, retry) {
            console.error("Project switcher render failed.", error)
            <button class={styles.switcherFallback} type="button" onClick={retry}>
              "Project switcher unavailable"
            </button>
          }
        </div>
      </div>
      <div class={cx(styles.actions, "electrobun-webkit-app-region-no-drag")}>
        <div class={styles.actionGroup}>
          <ActionButton
            ariaLabel="Propose task"
            icon={<tsrx>
              <InlineSvgIcon icon="what-next" size="17px 12px" />
            </tsrx>}
            onAction={AppCommand.navigation.openProposeTaskDialog}
          />
          <span class={styles.actionDivider} />
          <ActionButton
            ariaLabel="New session"
            icon={<tsrx>
              <InlineSvgIcon icon="new-session" size="16px 16px" />
            </tsrx>}
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
        items={mainTabItems.filter((item) => item.group === "primary")}
        onMainTabSelect={navigateMainTab}
        selectedMainTabId={mainTab.selectedKind}
      />
      <div aria-hidden="true" class={styles.navDivider} />
      <AppShellSidebarSection
        items={mainTabItems.filter((item) => item.group === "secondary")}
        onMainTabSelect={navigateMainTab}
        selectedMainTabId={mainTab.selectedKind}
      />
    </nav>
    <div class={styles.main}>
      <div ref={tabStrip.tabStripRef} class={styles.tabStrip}>
        <div class={styles.tabList} role="tablist" aria-label="Workbench tabs">
          <TabButton
            activeTabId={workbenchTabSet.activeTabId}
            icon="tabs/home"
            id={WORKBENCH_MAIN_TAB.id}
            isHome={true}
            onSelect={(id) => {
              workbenchTabSet.activateTab(id)
            }}
            refCallback={(element) => {
              tabStrip.tabRefs.current[WORKBENCH_MAIN_TAB.id] = element
            }}
            title={selectedMainTab?.label ?? WORKBENCH_MAIN_TAB.title}
          />

          for (const tab of workbenchTabSet.tabList) {
            <TabButton
              activeTabId={workbenchTabSet.activeTabId}
              dirty={tab.dirty}
              icon={getWorkbenchTabIcon(tab.kind)}
              id={tab.id}
              key={tab.id}
              onClose={(id) => {
                workbenchTabSet.closeTab(id)
              }}
              onDragEnd={() => {
                tabStrip.dragSourceTabId.current = null
              }}
              onDragEnter={(id) => {
                if (tabStrip.dragSourceTabId.current && tabStrip.dragSourceTabId.current !== id) {
                  workbenchTabSet.reorderTabs(tabStrip.dragSourceTabId.current, id)
                }
              }}
              onDragStart={(id) => {
                tabStrip.dragSourceTabId.current = id
              }}
              onSelect={(id) => {
                workbenchTabSet.activateTab(id)
              }}
              refCallback={(element) => {
                tabStrip.tabRefs.current[tab.id] = element
              }}
              title={tab.title}
            />
          }

          <span
            aria-hidden="true"
            class={styles.indicator}
            style={{
              opacity: tabStrip.indicator.opacity,
              transform: `translateX(${tabStrip.indicator.left}px)`,
              width: `${tabStrip.indicator.width}px`,
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
        <AppShellWorkbenchContent />
      </div>
    </div>
  </div>
}

/** Renders one grouped section of the left sidebar. */
component AppShellSidebarSection(props: {
  items: Array<MainTabItem & {
    icon: SvgIconName;
  }>;
  onMainTabSelect: (id: MainTabItemId) => void;
  selectedMainTabId: MainTabItemId;
}) {
  for (const item of props.items) {
    <SidebarItem
      dot={appShellSidebarDots[item.id] ?? appShellDefaultSidebarDot}
      icon={item.icon}
      isSelected={item.id === props.selectedMainTabId}
      key={item.id}
      label={item.label}
      onSelect={() => {
        props.onMainTabSelect(item.id)
      }}
    />
  }
}

component ActionButton(props: {
  ariaLabel: string;
  icon: preact.JSX.Element;
  onAction: () => void;
}) {
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
}

component SidebarItem(props: {
  dot: AppShellSidebarDot;
  icon: SvgIconName;
  isSelected: boolean;
  label: string;
  onSelect: () => void;
}) {
  const hasDot = props.dot.usePredicate()
  const ariaLabel =
    hasDot ? props.dot.getAriaLabel(props.label) : props.label

  <button
    aria-current={props.isSelected ? "page" : undefined}
    aria-label={ariaLabel}
    class={styles.sidebarItem}
    style={{
      color: props.isSelected ? textColor : `color-mix(in srgb, ${textColor} 38%, transparent)`,
    }}
    type="button"
    onClick={props.onSelect}
  >
    <InlineSvgIcon icon={props.icon} size="22px 21px" />
    if (hasDot) {
      <span aria-hidden="true" class={styles.sidebarUnreadDot} />
    }
  </button>
}

component TabButton(props: {
  activeTabId: string;
  dirty?: boolean;
  icon: SvgIconName;
  id: string;
  isHome?: boolean;
  onClose?: (id: string) => void;
  onDragEnd?: () => void;
  onDragEnter?: (id: string) => void;
  onDragStart?: (id: string) => void;
  onSelect: (id: string) => void;
  refCallback: (element: HTMLDivElement | null) => void;
  title: string;
}) {
  const isActive = props.activeTabId === props.id
  const isHome = props.isHome === true
  const [isHovered, setIsHovered] = useState(false)
  const showCloseButton = isActive || isHovered
  const iconColor =
    isActive
      ? isHome
        ? accentStrongColor
        : textColor
      : isHovered
        ? textColor
        : isHome
          ? `color-mix(in srgb, ${textColor} 52%, transparent)`
          : `color-mix(in srgb, ${textColor} 42%, transparent)`
  const labelColor =
    isActive
      ? isHome
        ? accentStrongColor
        : textColor
      : isHovered
        ? textColor
        : isHome
          ? `color-mix(in srgb, ${textColor} 52%, transparent)`
          : `color-mix(in srgb, ${textColor} 34%, transparent)`

  <div
    {ref props.refCallback}
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

    if (!isHome) {
      <div
        class={styles.tabTail}
        onClick={() => {
          props.onSelect(props.id)
        }}
      >
        if (props.dirty) {
          <span
            aria-hidden="true"
            class={styles.tabDirty}
            style={{
              opacity: showCloseButton ? "0" : "1",
            }}
          />
        }
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
    }
  </div>
}

component InlineSvgIcon(props: { icon: SvgIconName; size: string }) {
  const [width, height = width] = props.size.split(" ")

  <GoodIcon class={styles.icon} name={props.icon} {height} {width} />
}
