import { t } from "@lingui/core/macro"
import { combine } from "@atlaskit/pragmatic-drag-and-drop/combine"
import { draggable, dropTargetForElements } from "@atlaskit/pragmatic-drag-and-drop/element/adapter"
import { autoScrollForElements } from "@atlaskit/pragmatic-drag-and-drop-auto-scroll/element"
import {
  attachClosestEdge,
  extractClosestEdge,
  type Edge,
} from "@atlaskit/pragmatic-drag-and-drop-hitbox/closest-edge"
import { token } from "@goddard-ai/styled-system/tokens"
import { OverlayScrollbars } from "overlayscrollbars"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { GoodIcon, type SvgIconName } from "~/lib/good-icon.tsrx"
import { getWorkbenchTabIcon } from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB, type WorkbenchTab } from "~/workbench-tab-set.ts"
import styles from "./chrome.style.ts"

const accentStrongColor = token.var("colors.accentStrong")
const textColor = token.var("colors.text")
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

export function AppShellTabStrip(props: {
  activeTabId: string;
  endAccessory?: preact.ComponentChildren;
  selectedMainTabId: string;
  selectedMainTabTitle: string;
  tabs: readonly WorkbenchTab[];
  onActivateTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onMoveTab: (sourceId: string, targetId: string, placement: WorkbenchTabDragPlacement) => void;
}) @{
  const tabStrip = useAppShellTabStrip(props.activeTabId, props.selectedMainTabId, props.tabs)

  <div class={styles.tabStripRow}>
    <div ref={tabStrip.tabStripRef} class={styles.tabStrip} data-overlayscrollbars-initialize="">
      <div class={styles.tabList} role="tablist" aria-label={t`Workbench tabs`}>
        <TabButton
          activeTabId={props.activeTabId}
          icon="tabs/home"
          id={WORKBENCH_MAIN_TAB.id}
          isHome={true}
          onSelect={props.onActivateTab}
          refCallback={(element) => {
            tabStrip.tabRefs.current[WORKBENCH_MAIN_TAB.id] = element
          }}
          title={props.selectedMainTabTitle}
        />

        @for (const tab of props.tabs) {
          <TabButton
            activeTabId={props.activeTabId}
            dirty={tab.dirty}
            icon={getWorkbenchTabIcon(tab.kind)}
            id={tab.id}
            key={tab.id}
            onClose={props.onCloseTab}
            onSelect={props.onActivateTab}
            onTabDragEnd={tabStrip.hideDropIndicator}
            onTabDragOver={(id, placement) => {
              tabStrip.showDropIndicator(id, placement)
            }}
            onTabDrop={props.onMoveTab}
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

    {props.endAccessory}
  </div>
}

function TabButton(props: {
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
}) @{
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
      >{props.title}</span>
    </button>

    @if (!isHome) {
      <div
        class={styles.tabTail}
        onClick={() => {
          props.onSelect(props.id)
        }}
      >
        @if (props.dirty) {
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

function InlineSvgIcon(props: { icon: SvgIconName; size: string }) @{
  const [width, height = width] = props.size.split(" ")

  <GoodIcon class={styles.icon} name={props.icon} {height} {width} />
}
