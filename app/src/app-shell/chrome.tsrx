import { t } from "@lingui/core/macro"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { InboxItem } from "@goddard-ai/inbox/schema"
import { ChevronRight, Folder } from "lucide-react"
import { useEffect, useRef } from "preact/hooks"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { AppCommandTooltip } from "~/commands/app-command-tooltip.tsrx"
import { maximizeWindow } from "~/desktop-host.ts"
import { getNextUnreadInboxAttentionItem, hasUnreadSessionInboxItems } from "~/inbox/attention.ts"
import { NextInboxWorkbenchPreloader } from "~/inbox/next-preload.ts"
import {
  openInboxItemInWorkbench,
  prepareInboxItemWorkbenchTarget,
  warmPreparedInboxWorkbenchTargetOnIdle,
} from "~/inbox/open.ts"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsrx"
import { useQuery } from "~/lib/query.ts"
import type { MainTabItem, MainTabItemId } from "~/main-tab-items.ts"
import { SwitchProjectDropdown } from "~/projects/switch-project-dropdown.tsrx"
import { goddardSdk } from "~/sdk.ts"
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
export function AppShellChrome() @{
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()
  const topBarSelectorShortcutActive = workbenchTabSet.activeClosableTab?.kind !== "project"
  const selectedMainTab = mainTab.items.find((item) => item.id === mainTab.selectedKind)

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
          @try {
            <AppShellAgentSelector shortcutActive={topBarSelectorShortcutActive} />
          } @pending {
            <button
              aria-label={t`Loading agent harness selector`}
              class={cx(styles.switcherSideFallback, noDragClass)}
              disabled={true}
              type="button"
            />
          } @catch (error, retry) {
            useErrorBoundaryReset(retry)
            console.error("Agent harness selector render failed.", error)

            <button
              aria-label={t`Retry agent harness selector`}
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
          @try {
            <SwitchProjectDropdown />
          } @catch (error, retry) {
            useErrorBoundaryReset(retry)
            console.error("Project switcher render failed.", error)

            <button class={cx(styles.switcherFallback, noDragClass)} type="button" onClick={retry}>
              {t`Project switcher unavailable`}
            </button>
          }
        </div>
        <div class={styles.switcherRightControl}>
          @try {
            <AppShellSubfolderSelector shortcutActive={topBarSelectorShortcutActive} />
          } @pending {
            <button
              aria-label={t`Loading project subfolder selector`}
              class={cx(styles.switcherSideFallback, noDragClass)}
              disabled={true}
              type="button"
            >
              <Folder aria-hidden={true} size={16} strokeWidth={2.1} />
            </button>
          } @catch (error, retry) {
            useErrorBoundaryReset(retry)
            console.error("Project subfolder selector render failed.", error)

            <button
              aria-label={t`Retry project subfolder selector`}
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
          aria-label={t`Settings`}
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
    <nav aria-label={t`Primary`} class={styles.nav}>
      @try {
        <AppShellSidebarQueryContent
          items={mainTab.items}
          selectedMainTabId={mainTab.selectedKind}
        />
      } @pending {
        <AppShellSidebarContent
          hasUnreadInboxItems={false}
          items={mainTab.items}
          selectedMainTabId={mainTab.selectedKind}
        />
      } @catch (error, retry) {
        useErrorBoundaryReset(retry)
        console.error("Inbox unread indicator render failed.", error)

        <AppShellSidebarContent
          hasUnreadInboxItems={false}
          items={mainTab.items}
          selectedMainTabId={mainTab.selectedKind}
        />
      }
    </nav>

    <div class={styles.main}>
      <AppShellTabStrip
        activeTabId={workbenchTabSet.activeTabId}
        endAccessory={<AppShellNextInboxAttentionSlot {workbenchTabSet} />}
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
        @try {
          @if (workbenchTabSet.activeTabId === WORKBENCH_MAIN_TAB.id) {
            <WorkbenchScrollPanel scrollKey={`main:${mainTab.selectedKind}`}>
              <MainWorkbenchView />
            </WorkbenchScrollPanel>
          } @else if (workbenchTabSet.activeClosableTab) {
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
        } @pending {
        }
      </div>
    </div>
  </div>
}

function AppShellNextInboxAttentionSlot(props: {
  workbenchTabSet: ReturnType<typeof useWorkbenchTabSet>;
}) @{
  @try {
    <AppShellNextInboxAttentionButton workbenchTabSet={props.workbenchTabSet} />
  } @pending {
    <AppShellNextInboxAttentionShortcutFallback />
  } @catch (error, retry) {
    useErrorBoundaryReset(retry)
    console.error("Next inbox attention render failed.", error)

    <AppShellNextInboxAttentionShortcutFallback />
  }
}

function AppShellNextInboxAttentionShortcutFallback() @{
  useAppCommand(AppCommand.navigation.openNextUnreadInboxItem, () => {}, { active: false })

  <></>
}

function AppShellNextInboxAttentionButton(props: {
  workbenchTabSet: ReturnType<typeof useWorkbenchTabSet>;
}) @{
  const preloaderRef = useRef<NextInboxWorkbenchPreloader | null>(null)
  preloaderRef.current ??= new NextInboxWorkbenchPreloader({
    prepareTarget: prepareInboxItemWorkbenchTarget,
    warmTargetOnIdle: warmPreparedInboxWorkbenchTargetOnIdle,
  })
  const { items } = useQuery(goddardSdk.inbox.list, {
    params: [
      { statuses: ["unread"], limit: 100 },
    ],
  })
  const nextItem = getNextUnreadInboxAttentionItem(items)

  useEffect(() => {
    preloaderRef.current?.prepare(nextItem)
  }, [nextItem?.entityId, nextItem?.id, nextItem?.updatedAt])

  async function openNextInboxItem(item: InboxItem) {
    try {
      const preparedTarget = await preloaderRef.current?.resolve(item) ?? null
      await openInboxItemInWorkbench({
        item,
        preparedTarget,
        workbenchTabSet: props.workbenchTabSet,
      })
    } catch (error) {
      console.error("Failed to open next inbox item.", error)
      appToaster.create({
        description: t`The linked daemon entity could not be loaded.`,
        duration: 3200,
        title: t`Couldn't open next item`,
        type: "error",
      })
    }
  }

  useAppCommand(AppCommand.navigation.openNextUnreadInboxItem, () => {
    if (nextItem) {
      void openNextInboxItem(nextItem)
    }
  }, { active: nextItem !== null })

  @if (nextItem) {
    <AppCommandTooltip
      command={AppCommand.navigation.openNextUnreadInboxItem}
      group="app-shell-next-attention"
      label={t`Next`}
      side="bottom"
      sideOffset={8}
    >
      <button
        aria-label={t`Open next unread inbox item`}
        class={cx(styles.nextAttentionButton, noDragClass)}
        type="button"
        onClick={() => {
          void openNextInboxItem(nextItem)
        }}
      >
        <span>
          {t`Next`}
        </span>
        <ChevronRight aria-hidden={true} size={14} strokeWidth={2.2} />
      </button>
    </AppCommandTooltip>
  } @else {
    <></>
  }
}

function AppShellSidebarQueryContent(props: {
  items: readonly MainTabItem[];
  selectedMainTabId: MainTabItemId;
}) @{
  const { items } = useQuery(goddardSdk.inbox.list, {
    params: [
      { statuses: ["unread"], limit: 100 },
    ],
  })

  <AppShellSidebarContent
    hasUnreadInboxItems={hasUnreadSessionInboxItems(items)}
    items={props.items}
    selectedMainTabId={props.selectedMainTabId}
  />
}

function AppShellSidebarContent(props: {
  hasUnreadInboxItems: boolean;
  items: readonly MainTabItem[];
  selectedMainTabId: MainTabItemId;
}) @{
  const sidebarState: AppShellSidebarState = {
    hasUnreadInboxItems: props.hasUnreadInboxItems,
  }
  const mainTabItems = props.items.map(
    (item) => ({
      ...item,
      icon: getWorkbenchTabIcon(item.id),
    }),
  )

  <>
    <AppShellSidebarSection
      items={mainTabItems.filter((item) => item.group === "primary")}
      {sidebarState}
      selectedMainTabId={props.selectedMainTabId}
    />
    <div aria-hidden="true" class={styles.navDivider} />
    <AppShellSidebarSection
      items={mainTabItems.filter((item) => item.group === "secondary")}
      {sidebarState}
      selectedMainTabId={props.selectedMainTabId}
    />
  </>
}

/** Renders one grouped section of the left sidebar. */
function AppShellSidebarSection(props: {
  items: Array<MainTabItem & {
    icon: SvgIconName;
  }>;
  sidebarState: AppShellSidebarState;
  selectedMainTabId: MainTabItemId;
}) @{
  @for (const item of props.items) {
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

function ActionButton(props: {
  ariaLabel: string;
  icon: preact.JSX.Element;
  onAction: () => void;
}) @{
  <button
    aria-label={props.ariaLabel}
    class={cx(styles.actionButton, styles.actionItem, noDragClass)}
    type="button"
    onClick={() => {
      props.onAction()
    }}
  >{props.icon}</button>
}

function SidebarItem(props: {
  command: AppCommand;
  dot: AppShellSidebarDot;
  icon: SvgIconName;
  isSelected: boolean;
  label: string;
  sidebarState: AppShellSidebarState;
}) @{
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

      @if (hasDot) {
        <span aria-hidden="true" class={styles.sidebarUnreadDot} />
      }
    </button>
  </AppCommandTooltip>
}

function InlineSvgIcon(props: { icon: SvgIconName; size: string }) @{
  const [width, height = width] = props.size.split(" ")

  <GoodIcon class={styles.icon} name={props.icon} {height} {width} />
}
