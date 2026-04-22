import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { useNavigation, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { maximizeWindow } from "~/desktop-host.ts"
import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsrx"
import type { NavigationItemId } from "~/navigation.ts"
import { SwitchProjectDropdown } from "~/projects/switch-project-dropdown.tsrx"
import { getWorkbenchTabIcon } from "~/workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "~/workbench-tab-set.ts"
import styles from "./chrome.style.ts"
import { appShellSections } from "./config.ts"
import { AppShellWorkbenchContent } from "./views.tsrx"

const accentStrongColor = token.var("colors.accentStrong")
const textColor = token.var("colors.text")

function useAppShellTabStrip(
  activeTabId: string,
  selectedNavigationId: string,
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

    setIndicator(
      {
        left: activeTabElement.offsetLeft,
        width: activeTabElement.offsetWidth,
        opacity: 1,
      },
    )
  }

  useLayoutEffect(() => {
    syncIndicator()
  }, [activeTabId, selectedNavigationId, tabs])

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
  }, [selectedNavigationId, tabs])

  return {
    dragSourceTabId,
    indicator,
    tabRefs,
    tabStripRef,
  }
}

/** Renders the merged shell chrome and owns its local interaction wiring. */
export component AppShellChrome() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()
  const navigationItems: Array<{
    group: "primary" | "secondary";
    icon: SvgIconName;
    id: NavigationItemId;
    label: string;
  }> = navigation.items.map((item) => {
    const section = appShellSections.find((candidate) => candidate.tabKinds.includes(item.id))

    if (!section) {
      throw new Error(`Missing app shell section for navigation item ${item.id}.`)
    }

    return {
      ...item,
      group: section.group,
      icon: getWorkbenchTabIcon(item.id),
    }
  })
  const selectedNavigation =
    navigationItems.find((item) => item.id === navigation.selectedNavId) ?? navigationItems[0]
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    navigation.selectedNavId,
    workbenchTabSet.tabList,
  )

  function openNavigationSurfaceTab(kind: NavigationItemId) {
    const nextNavigationItem =
      navigation.items.find((item) => item.id === kind) ?? navigation.selectedItem

    workbenchTabSet.openOrFocusTab(
      {
        id: `surface:${kind}`,
        kind,
        title: nextNavigationItem.label,
        payload: {},
        dirty: false,
      },
    )
  }

  function selectNavigationSurface(id: NavigationItemId, options?: { openInTab?: boolean }) {
    if (options?.openInTab) {
      openNavigationSurfaceTab(id)
      return
    }

    navigation.selectNavItem(id)
    workbenchTabSet.activateTab(WORKBENCH_PRIMARY_TAB.id)
  }

  useAppCommand(AppCommand.workbench.closeActiveTab, () => {
    if (workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useAppCommand(AppCommand.navigation.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab(
      {
        id: "workbench:keyboard-shortcuts",
        kind: "keyboardShortcuts",
        title: "Keyboard Shortcuts",
        payload: {},
        dirty: false,
      },
    )
  })

  useAppCommand(AppCommand.navigation.openInbox, () => {
    selectNavigationSurface("inbox")
  })

  useAppCommand(AppCommand.navigation.openSessions, () => {
    selectNavigationSurface("sessions")
  })

  useAppCommand(AppCommand.navigation.openSearch, () => {
    selectNavigationSurface("search")
  })

  useAppCommand(AppCommand.navigation.openSpecs, () => {
    selectNavigationSurface("specs")
  })

  useAppCommand(AppCommand.navigation.openTasks, () => {
    selectNavigationSurface("tasks")
  })

  useAppCommand(AppCommand.navigation.openRoadmap, () => {
    selectNavigationSurface("roadmap")
  })

  useAppCommand(AppCommand.navigation.openSettings, () => {
    workbenchTabSet.openOrFocusTab(
      {
        id: "surface:settings",
        kind: "settings",
        title: "Settings",
        payload: {},
        dirty: false,
      },
    )
  })

  <tsx:react>
    <div class={styles.root}>
      <header class={cx(styles.header, "electrobun-webkit-app-region-drag")} onDblClick={(
        event,
      ) => {
        if (
          (event.target as HTMLElement | null)?.closest(".electrobun-webkit-app-region-no-drag")
        ) {
          return
        }

        void maximizeWindow()
      }}>
        <div class={cx(styles.switcher, "electrobun-webkit-app-region-no-drag")}>
          <div class={css(
            {
              position: "relative",
              width: "100%",
            },
          )}>
            <SwitchProjectDropdown />
          </div>
        </div>
        <div class={cx(styles.actions, "electrobun-webkit-app-region-no-drag")}>
          <div class={styles.actionGroup}>
            <ActionButton ariaLabel="Propose task" icon={<tsx:react>
              <InlineSvgIcon icon="what-next" size="17px 12px" />
            </tsx:react>} onAction={AppCommand.navigation.openProposeTaskDialog} />
            <span class={styles.actionDivider} />
            <ActionButton ariaLabel="New session" icon={<tsx:react>
              <InlineSvgIcon icon="new-session" size="16px 16px" />
            </tsx:react>} onAction={AppCommand.navigation.openNewSessionDialog} />
          </div>
          <button aria-label="Settings" class={cx(
            styles.actionButton,
            styles.settingsButton,
          )} type="button" onClick={() => {
            AppCommand.navigation.openSettings()
          }}>
            <InlineSvgIcon icon="settings" size="15px" />
          </button>
        </div>
      </header>
      <nav aria-label="Primary" class={styles.nav}>
        <AppShellSidebarSection items={navigationItems.filter(
          (item) => item.group === "primary",
        )} onNavigationSelect={selectNavigationSurface} selectedNavigationId={navigation.selectedNavId} />
        <div aria-hidden="true" class={styles.navDivider} />
        <AppShellSidebarSection items={navigationItems.filter(
          (item) => item.group === "secondary",
        )} onNavigationSelect={selectNavigationSurface} selectedNavigationId={navigation.selectedNavId} />
      </nav>
      <div class={styles.main}>
        <div ref={tabStrip.tabStripRef} class={styles.tabStrip}>
          <div class={styles.tabList} role="tablist" aria-label="Workbench tabs">
            <TabButton activeTabId={workbenchTabSet.activeTabId} icon={getWorkbenchTabIcon(
              "main",
            )} id={WORKBENCH_PRIMARY_TAB.id} isHome={true} onSelect={(id) => {
              workbenchTabSet.activateTab(id)
            }} refCallback={(element) => {
              tabStrip.tabRefs.current[WORKBENCH_PRIMARY_TAB.id] = element
            }} title={selectedNavigation?.label ?? WORKBENCH_PRIMARY_TAB.title} />
            {workbenchTabSet.tabList.map(
              (tab) => <tsx:react>
                <TabButton key={tab.id} activeTabId={workbenchTabSet.activeTabId} dirty={tab.dirty} icon={getWorkbenchTabIcon(
                  tab.kind,
                )} id={tab.id} onClose={(id) => {
                  workbenchTabSet.closeTab(id)
                }} onDragEnd={() => {
                  tabStrip.dragSourceTabId.current = null
                }} onDragEnter={(id) => {
                  if (tabStrip.dragSourceTabId.current && tabStrip.dragSourceTabId.current !== id) {
                    workbenchTabSet.reorderTabs(tabStrip.dragSourceTabId.current, id)
                  }
                }} onDragStart={(id) => {
                  tabStrip.dragSourceTabId.current = id
                }} onSelect={(id) => {
                  workbenchTabSet.activateTab(id)
                }} refCallback={(element) => {
                  tabStrip.tabRefs.current[tab.id] = element
                }} title={tab.title} />
              </tsx:react>,
            )}
            <span aria-hidden="true" class={styles.indicator} style={{
              opacity: tabStrip.indicator.opacity,
              transform: `translateX(${tabStrip.indicator.left}px)`,
              width: `${tabStrip.indicator.width}px`,
            }} />
          </div>
        </div>
        <div class={css(
          {
            minHeight: "0",
            overflow: "hidden",
          },
        )}>
          <AppShellWorkbenchContent />
        </div>
      </div>
    </div>
  </tsx:react>
}

/** Renders one grouped section of the left navigation rail. */
component AppShellSidebarSection(props: {
  items: Array<{
    icon: SvgIconName;
    id: NavigationItemId;
    label: string;
  }>;
  onNavigationSelect: (id: NavigationItemId, options?: {
    openInTab?: boolean;
  }) => void;
  selectedNavigationId: NavigationItemId;
}) {
  <tsx:react>
    <>
      {props.items.map(
        (item) => <tsx:react>
          <SidebarItem key={item.id} icon={item.icon} isSelected={item.id ===
            props.selectedNavigationId} label={item.label} onSelect={(options) => {
            props.onNavigationSelect(item.id, options)
          }} />
        </tsx:react>,
      )}
    </>
  </tsx:react>
}

component ActionButton(props: {
  ariaLabel: string;
  icon: preact.JSX.Element;
  onAction: () => void;
}) {
  <tsx:react>
    <button aria-label={props.ariaLabel} class={cx(
      styles.actionButton,
      styles.actionItem,
    )} type="button" onClick={() => {
      props.onAction()
    }}>
      {props.icon}
    </button>
  </tsx:react>
}

component SidebarItem(props: {
  icon: SvgIconName;
  isSelected: boolean;
  label: string;
  onSelect: (options?: { openInTab?: boolean }) => void;
}) {
  <tsx:react>
    <button aria-current={props.isSelected
      ? "page"
      : undefined} aria-label={props.label} class={styles.sidebarItem} style={{
      color: props.isSelected ? textColor : `color-mix(in srgb, ${textColor} 38%, transparent)`,
    }} type="button" onClick={(event) => {
      props.onSelect({ openInTab: event.metaKey })
    }}>
      <InlineSvgIcon icon={props.icon} size="22px 21px" />
    </button>
  </tsx:react>
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

  <tsx:react>
    <div ref={props.refCallback} class={styles.tab} draggable={!isHome} role="presentation" onDragEnd={() => {
      props.onDragEnd?.()
    }} onDragEnter={() => {
      props.onDragEnter?.(props.id)
    }} onDragOver={(event) => {
      if (!isHome) {
        event.preventDefault()
      }
    }} onDragStart={() => {
      props.onDragStart?.(props.id)
    }} onMouseEnter={() => {
      setIsHovered(true)
    }} onMouseLeave={() => {
      setIsHovered(false)
    }}>
      <button aria-selected={isActive} class={styles.tabButton} style={{
        paddingLeft: "8px",
        paddingRight: isHome ? "14px" : "8px",
      }} role="tab" type="button" onClick={() => {
        props.onSelect(props.id)
      }}>
        <span class={styles.tabIcon} style={{
          color: iconColor,
        }}>
          <InlineSvgIcon icon={props.icon} size="14px" />
        </span>
        <span class={styles.tabLabel} style={{
          color: labelColor,
        }}>
          {props.title}
        </span>
      </button>
      {isHome
        ? null
        : <tsx:react>
            <div class={styles.tabTail} onClick={() => {
              props.onSelect(props.id)
            }}>
              {props.dirty
                ? <tsx:react>
                    <span aria-hidden="true" class={styles.tabDirty} style={{
                      opacity: showCloseButton ? "0" : "1",
                    }} />
                  </tsx:react>
                : null}
              <button aria-label={`Close ${props.title}`} class={styles.tabClose} style={{
                opacity: showCloseButton ? "1" : "0",
                pointerEvents: showCloseButton ? "auto" : "none",
              }} type="button" onClick={(event) => {
                event.stopPropagation()
                props.onClose?.(props.id)
              }}>
                <InlineSvgIcon icon="close-tab" size="9px" />
              </button>
            </div>
          </tsx:react>}
    </div>
  </tsx:react>
}

component InlineSvgIcon(props: { icon: SvgIconName; size: string }) {
  const [width, height = width] = props.size.split(" ")

  <tsx:react>
    <GoodIcon class={styles.icon} name={props.icon} height={height} width={width} />
  </tsx:react>
}
