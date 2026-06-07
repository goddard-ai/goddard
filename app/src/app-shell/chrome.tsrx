import { combine } from "@atlaskit/pragmatic-drag-and-drop/combine"
import { draggable, dropTargetForElements } from "@atlaskit/pragmatic-drag-and-drop/element/adapter"
import { autoScrollForElements } from "@atlaskit/pragmatic-drag-and-drop-auto-scroll/element"
import {
  attachClosestEdge,
  extractClosestEdge,
  type Edge,
} from "@atlaskit/pragmatic-drag-and-drop-hitbox/closest-edge"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Folder } from "lucide-react"
import { OverlayScrollbars } from "overlayscrollbars"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { useInbox, useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { AppCommandTooltip } from "~/commands/app-command-tooltip.tsrx"
import { maximizeWindow } from "~/desktop-host.ts"
import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsrx"
import type { MainTabItem, MainTabItemId } from "~/main-tab-items.ts"
import { SwitchProjectDropdown } from "~/projects/switch-project-dropdown.tsrx"
import { getWorkbenchTabIcon, shouldRestoreWorkbenchTabScroll } from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB } from "~/workbench-tab-set.ts"
import { AppShellAgentSelector } from "./agent-selector.tsrx"
import styles from "./chrome.style.ts"
import {
  appShellDefaultSidebarDot,
  appShellSidebarDots,
  type AppShellSidebarDot,
  type AppShellSidebarState,
} from "./config.ts"
import { AppShellSubfolderSelector } from "./subfolder-selector.tsrx"
import { MainWorkbenchView, WorkbenchScrollPanel, WorkbenchTabPanel } from "./views.tsrx"

const accentStrongColor = token.var("colors.accentStrong")
const textColor = token.var("colors.text")
const noDragClass = "electrobun-webkit-app-region-no-drag"
const workbenchTabDragDataType = "workbench-tab"

type WorkbenchTabDragPlacement = "before" | "after"

function getWorkbenchTabDragId(data: Record<string, unknown>) {
  if (data.type === workbenchTabDragDataType && typeof data.tabId === "string") {
    return data.tabId
  }

  return null
}

function getWorkbenchTabDragPlacement(data: Record<string | symbol, unknown>) {
  const edge = extractClosestEdge(data)

  if (edge === "right") {
    return "after"
  }

  if (edge === "left") {
    return "before"
  }

  return null
}

const mainTabCommands: Record<MainTabItemId, AppCommand> = {
  inbox: AppCommand.navigation.openInbox,
  sessions: AppCommand.navigation.openSessions,
  search: AppCommand.navigation.openSearch,
  specs: AppCommand.navigation.openSpecs,
  tasks: AppCommand.navigation.openTasks,
  roadmap: AppCommand.navigation.openRoadmap,
}

function useAppShellTabStrip(
  activeTabId: string,
  selectedMainTabId: string,
  tabs: readonly { id: string }[],
) {
  const tabStripRef = useRef<HTMLDivElement | null>(null)
  const tabRefs = useRef<Record<string, HTMLDivElement | null>>({})
  const scrollbarRef = useRef<ReturnType<typeof OverlayScrollbars> | null>(null)
  const [indicator, setIndicator] = useState({ left: 0, width: 0, opacity: 0 })
  const [dropIndicator, setDropIndicator] = useState({ left: 0, opacity: 0 })

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

  function scrollActiveTabIntoView() {
    const activeTabElement = tabRefs.current[activeTabId]
    const tabStripElement = tabStripRef.current

    if (!activeTabElement || !tabStripElement) {
      return
    }

    const scrollElement = scrollbarRef.current?.elements().viewport ?? tabStripElement
    const activeTabRect = activeTabElement.getBoundingClientRect()
    const scrollRect = scrollElement.getBoundingClientRect()

    if (activeTabRect.left < scrollRect.left) {
      scrollElement.scrollTo({
        left: scrollElement.scrollLeft + activeTabRect.left - scrollRect.left,
        behavior: "smooth",
      })
    } else if (activeTabRect.right > scrollRect.right) {
      scrollElement.scrollTo({
        left: scrollElement.scrollLeft + activeTabRect.right - scrollRect.right,
        behavior: "smooth",
      })
    }
  }

  function showDropIndicator(targetTabId: string, placement: WorkbenchTabDragPlacement) {
    const targetElement = tabRefs.current[targetTabId]

    if (!targetElement) {
      setDropIndicator((current) => current.opacity === 0 ? current : { ...current, opacity: 0 })
      return
    }

    const left =
      placement === "after"
        ? targetElement.offsetLeft + targetElement.offsetWidth
        : targetElement.offsetLeft

    setDropIndicator(
      (current) => (current.left === left && current.opacity === 1
        ? current
        : { left, opacity: 1 }),
    )
  }

  function hideDropIndicator() {
    setDropIndicator((current) => current.opacity === 0 ? current : { ...current, opacity: 0 })
  }

  useLayoutEffect(() => {
    syncIndicator()
    scrollActiveTabIntoView()
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

  useEffect(() => {
    const tabStripElement = tabStripRef.current

    if (!tabStripElement) {
      return
    }

    const scrollbar = OverlayScrollbars(tabStripElement, {
      overflow: {
        x: "scroll",
        y: "hidden",
      },
      scrollbars: {
        autoHide: "move",
        theme: "os-theme-goddard-workbench-tabs",
        visibility: "auto",
      },
    })

    scrollbarRef.current = scrollbar
    const cleanupAutoScroll = autoScrollForElements({
      element: scrollbar.elements().viewport ?? tabStripElement,
      canScroll: ({ source }) => getWorkbenchTabDragId(source.data) !== null,
      getAllowedAxis: () => "horizontal",
    })
    scrollActiveTabIntoView()

    return () => {
      cleanupAutoScroll()
      scrollbarRef.current = null
      scrollbar.destroy()
    }
  }, [])

  return {
    dropIndicator,
    hideDropIndicator,
    indicator,
    showDropIndicator,
    tabRefs,
    tabStripRef,
  }
}

/** Renders the merged shell chrome and owns its local interaction wiring. */
export component AppShellChrome() {
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()

  function navigateMainTab(id: MainTabItemId) {
    mainTab.selectKind(id)
    workbenchTabSet.activateTab(WORKBENCH_MAIN_TAB.id)
  }

  useAppCommand(AppCommand.workbench.closeActiveTab, () => {
    if (workbenchTabSet.activeTabId !== WORKBENCH_MAIN_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useAppCommand(AppCommand.navigation.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab({
      kind: "keyboardShortcuts",
      props: {},
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
      props: {},
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
        <div class={styles.switcherLeftControl}>
          try {
            <AppShellAgentSelector />
          } pending {
            <button
              aria-label="Loading ACP agent selector"
              class={cx(styles.switcherSideFallback, noDragClass)}
              disabled={true}
              type="button"
            />
          } catch (error, retry) {
            console.error("ACP agent selector render failed.", error)

            <button
              aria-label="Retry ACP agent selector"
              class={cx(styles.switcherSideFallback, noDragClass)}
              type="button"
              onClick={retry}
            />
          }
        </div>
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

            <button class={cx(styles.switcherFallback, noDragClass)} type="button" onClick={retry}>
              "Project switcher unavailable"
            </button>
          }
        </div>
        <div class={styles.switcherRightControl}>
          try {
            <AppShellSubfolderSelector />
          } pending {
            <button
              aria-label="Loading project subfolder selector"
              class={cx(styles.switcherSideFallback, noDragClass)}
              disabled={true}
              type="button"
            >
              <Folder aria-hidden={true} size={16} strokeWidth={2.1} />
            </button>
          } catch (error, retry) {
            console.error("Project subfolder selector render failed.", error)

            <button
              aria-label="Retry project subfolder selector"
              class={cx(styles.switcherSideFallback, noDragClass)}
              type="button"
              onClick={retry}
            >
              <Folder aria-hidden={true} size={16} strokeWidth={2.1} />
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
          class={cx(styles.actionButton, styles.settingsButton, noDragClass)}
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
      const inbox = useInbox()
      const sidebarState: AppShellSidebarState = { inbox }
      const mainTabItems = mainTab.items.map(
        (item) => ({
          ...item,
          icon: getWorkbenchTabIcon(item.id),
        }),
      )

      <AppShellSidebarSection
        items={mainTabItems.filter((item) => item.group === "primary")}
        {sidebarState}
        selectedMainTabId={mainTab.selectedKind}
      />
      <div aria-hidden="true" class={styles.navDivider} />
      <AppShellSidebarSection
        items={mainTabItems.filter((item) => item.group === "secondary")}
        {sidebarState}
        selectedMainTabId={mainTab.selectedKind}
      />
    </nav>

    const selectedMainTab = mainTab.items.find((item) => item.id === mainTab.selectedKind)
    const tabStrip = useAppShellTabStrip(
      workbenchTabSet.activeTabId,
      mainTab.selectedKind,
      workbenchTabSet.tabList,
    )

    <div class={styles.main}>
      <div ref={tabStrip.tabStripRef} class={styles.tabStrip} data-overlayscrollbars-initialize="">
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
              onSelect={(id) => {
                workbenchTabSet.activateTab(id)
              }}
              onTabDragEnd={tabStrip.hideDropIndicator}
              onTabDragOver={(id, placement) => {
                tabStrip.showDropIndicator(id, placement)
              }}
              onTabDrop={(sourceId, targetId, placement) => {
                workbenchTabSet.moveTab(sourceId, targetId, placement)
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
          <span
            aria-hidden="true"
            class={styles.tabDropIndicator}
            style={{
              opacity: tabStrip.dropIndicator.opacity,
              transform: `translateX(${tabStrip.dropIndicator.left - 1}px)`,
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
        try {
          if (workbenchTabSet.activeTabId === WORKBENCH_MAIN_TAB.id) {
            <WorkbenchScrollPanel scrollKey={`main:${mainTab.selectedKind}`}>
              <MainWorkbenchView />
            </WorkbenchScrollPanel>
          } else if (workbenchTabSet.activeClosableTab) {
            <WorkbenchScrollPanel
              restoreScroll={shouldRestoreWorkbenchTabScroll(
                workbenchTabSet.activeClosableTab.kind,
              )}
              scrollKey={`detail:${workbenchTabSet.activeClosableTab.id}`}
              tabId={workbenchTabSet.activeClosableTab.id}
            >
              <WorkbenchTabPanel />
            </WorkbenchScrollPanel>
          }
        } pending {}
      </div>
    </div>
  </div>
}

/** Renders one grouped section of the left sidebar. */
component AppShellSidebarSection(props: {
  items: Array<MainTabItem & {
    icon: SvgIconName;
  }>;
  sidebarState: AppShellSidebarState;
  selectedMainTabId: MainTabItemId;
}) {
  for (const item of props.items) {
    const command = mainTabCommands[item.id]

    <SidebarItem
      {command}
      dot={appShellSidebarDots[item.id] ?? appShellDefaultSidebarDot}
      icon={item.icon}
      isSelected={item.id === props.selectedMainTabId}
      key={item.id}
      label={item.label}
      sidebarState={props.sidebarState}
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
    class={cx(styles.actionButton, styles.actionItem, noDragClass)}
    type="button"
    onClick={() => {
      props.onAction()
    }}
  >
    {props.icon}
  </button>
}

component SidebarItem(props: {
  command: AppCommand;
  dot: AppShellSidebarDot;
  icon: SvgIconName;
  isSelected: boolean;
  label: string;
  sidebarState: AppShellSidebarState;
}) {
  const hasDot = props.dot.isVisible(props.sidebarState)
  const ariaLabel =
    hasDot ? props.dot.getAriaLabel(props.label) : props.label

  <AppCommandTooltip
    command={props.command}
    group="app-shell-sidebar"
    label={props.label}
    side="right"
    sideOffset={10}
  >
    <button
      aria-current={props.isSelected ? "page" : undefined}
      aria-label={ariaLabel}
      class={styles.sidebarItem}
      style={{
        color: props.isSelected ? textColor : `color-mix(in srgb, ${textColor} 38%, transparent)`,
      }}
      type="button"
      onClick={() => {
        props.command()
      }}
    >
      <InlineSvgIcon icon={props.icon} size="22px 21px" />

      if (hasDot) {
        <span aria-hidden="true" class={styles.sidebarUnreadDot} />
      }
    </button>
  </AppCommandTooltip>
}

component TabButton(props: {
  activeTabId: string;
  dirty?: boolean;
  icon: SvgIconName;
  id: string;
  isHome?: boolean;
  onClose?: (id: string) => void;
  onSelect: (id: string) => void;
  onTabDragEnd?: () => void;
  onTabDragOver?: (id: string, placement: WorkbenchTabDragPlacement) => void;
  onTabDrop?: (sourceId: string, targetId: string, placement: WorkbenchTabDragPlacement) => void;
  refCallback: (element: HTMLDivElement | null) => void;
  title: string;
}) {
  const isActive = props.activeTabId === props.id
  const isHome = props.isHome === true
  const [isHovered, setIsHovered] = useState(false)
  const [isDragging, setIsDragging] = useState(false)
  const tabRef = useRef<HTMLDivElement | null>(null)
  const tabButtonRef = useRef<HTMLButtonElement | null>(null)
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

  useEffect(() => {
    const element = tabRef.current
    const dragHandle = tabButtonRef.current

    if (isHome || !element || !dragHandle) {
      return
    }

    function notifyDragOver(data: Record<string | symbol, unknown>) {
      const placement = getWorkbenchTabDragPlacement(data)

      if (placement) {
        props.onTabDragOver?.(props.id, placement)
      }
    }

    return combine(
      draggable({
        element,
        dragHandle,
        getInitialData: () => ({
          type: workbenchTabDragDataType,
          tabId: props.id,
        }),
        onDragStart: () => {
          setIsDragging(true)
        },
        onDrop: () => {
          setIsDragging(false)
          props.onTabDragEnd?.()
        },
      }),
      dropTargetForElements({
        element,
        canDrop: ({ source }) => {
          const sourceTabId = getWorkbenchTabDragId(source.data)

          return sourceTabId !== null && sourceTabId !== props.id
        },
        getData: ({ input, element }) => attachClosestEdge(
          {
            type: workbenchTabDragDataType,
            tabId: props.id,
          },
          {
            element,
            input,
            allowedEdges: ["left", "right"] satisfies Edge[],
          },
        ),
        getDropEffect: () => "move",
        getIsSticky: () => true,
        onDrag: ({ self }) => {
          notifyDragOver(self.data)
        },
        onDragEnter: ({ self }) => {
          notifyDragOver(self.data)
        },
        onDragLeave: () => {
          props.onTabDragEnd?.()
        },
        onDrop: ({ source, self }) => {
          const placement = getWorkbenchTabDragPlacement(self.data)
          const sourceTabId = getWorkbenchTabDragId(source.data)

          if (placement && sourceTabId) {
            props.onTabDrop?.(sourceTabId, props.id, placement)
          }
        },
      }),
    )
  }, [isHome, props.id])

  <div
    ref={(element) => {
      tabRef.current = element
      props.refCallback(element)
    }}
    class={styles.tab}
    role="presentation"
    style={{
      opacity: isDragging ? "0.45" : "1",
    }}
    onMouseEnter={() => {
      setIsHovered(true)
    }}
    onMouseLeave={() => {
      setIsHovered(false)
    }}
  >
    <button
      ref={tabButtonRef}
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
