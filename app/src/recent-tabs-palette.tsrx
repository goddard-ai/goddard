import { t } from "@lingui/core/macro"
import type { Signal } from "@preact/signals"

import { useWorkbenchTabSet } from "./app-state-context.tsrx"
import styles from "./recent-tabs-palette.style.ts"
import { GoodIcon } from "./lib/good-icon.tsrx"
import { GoodPaletteModal, type GoodPaletteModalItem } from "./lib/good-palette-modal.tsrx"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"

export default function RecentTabsPalette(props: { open: Signal<boolean> }) @{
  const workbenchTabSet = useWorkbenchTabSet()
  const items = workbenchTabSet.recentDetailTabList.filter(
    (tab) => tab.id !== workbenchTabSet.activeTabId,
  ).map((tab): GoodPaletteModalItem => {
    const icon = getWorkbenchTabIcon(tab.kind)

    return {
      id: tab.id,
      icon: <GoodIcon name={icon} height="16px" width="16px" />,
      keywords: [tab.kind],
      label: tab.title,
      suffix: <span class={styles.itemMeta}>
        {workbenchTabSet.tabs[tab.id] ? t`Open` : t`Closed`}
      </span>,
    }
  })

  function selectRecentTab(item: GoodPaletteModalItem) {
    props.open.value = false
    workbenchTabSet.openOrFocusRecentTab(item.id)
  }

  <GoodPaletteModal
    emptyText={t`No recent tabs.`}
    {items}
    listLabel={t`Recent tabs`}
    open={props.open}
    placeholder={t`Type a tab`}
    title={t`Recent tabs`}
    onSelect={selectRecentTab}
  />
}
