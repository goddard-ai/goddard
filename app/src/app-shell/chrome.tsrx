import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Folder } from "lucide-react"

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
import { AppShellTabStrip } from "./tab-strip.tsrx"
import { MainWorkbenchView, WorkbenchScrollPanel, WorkbenchTabPanel } from "./views.tsrx"

const textColor = token.var("colors.text")
const noDragClass = "electrobun-webkit-app-region-no-drag"

const mainTabCommands: Record<MainTabItemId, AppCommand> = {
  inbox: AppCommand.navigation.openInbox,
  sessions: AppCommand.navigation.openSessions,
  search: AppCommand.navigation.openSearch,
  specs: AppCommand.navigation.openSpecs,
  tasks: AppCommand.navigation.openTasks,
  roadmap: AppCommand.navigation.openRoadmap,
}

/** Renders the merged shell chrome and owns its local interaction wiring. */
export component AppShellChrome() {
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()
  const topBarSelectorShortcutActive = workbenchTabSet.activeClosableTab?.kind !== "project"

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
            <AppShellAgentSelector shortcutActive={topBarSelectorShortcutActive} />
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
            <AppShellSubfolderSelector shortcutActive={topBarSelectorShortcutActive} />
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

    <div class={styles.main}>
      <AppShellTabStrip
        activeTabId={workbenchTabSet.activeTabId}
        selectedMainTabId={mainTab.selectedKind}
        selectedMainTabTitle={selectedMainTab?.label ?? WORKBENCH_MAIN_TAB.title}
        tabs={workbenchTabSet.tabList}
        onActivateTab={(id) => {
          workbenchTabSet.activateTab(id)
        }}
        onCloseTab={(id) => {
          workbenchTabSet.closeTab(id)
        }}
        onMoveTab={(sourceId, targetId, placement) => {
          workbenchTabSet.moveTab(sourceId, targetId, placement)
        }}
      />
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

component InlineSvgIcon(props: { icon: SvgIconName; size: string }) {
  const [width, height = width] = props.size.split(" ")

  <GoodIcon class={styles.icon} name={props.icon} {height} {width} />
}
