import {
  layoutWithLines,
  prepareWithSegments,
  type LayoutLine,
  type PrepareOptions,
  type PreparedTextWithSegments,
} from "@chenglou/pretext"
import { css } from "@goddard-ai/styled-system/css"
import type { ComponentChild } from "preact"
import { useEffect, useLayoutEffect, useMemo, useRef } from "preact/hooks"
import { usePretextViewport } from "./PretextViewport"

const listCanvasClass = css({
  position: "relative",
  minHeight: "100%",
})

const loadingOverlayClass = css({
  position: "absolute",
  inset: "0",
  display: "grid",
  placeItems: "center",
  color: "muted",
  fontSize: "0.94rem",
  letterSpacing: "0.01em",
  pointerEvents: "none",
})

const preparedParagraphCache = new Map<string, PreparedTextWithSegments>()
const paragraphScrollCache = new Map<string, number>()

/** One measured paragraph produced from Pretext layout. */
export type PretextParagraphMeasurement = {
  lines: readonly LayoutLine[]
  lineCount: number
  height: number
  maxLineWidth: number
}

/** One paragraph description consumed by the shared virtualized paragraph list. */
export type PretextParagraphSpec = {
  key: string
  text: string
  font: string
  lineHeight: number
  maxWidth: number
  whiteSpace?: PrepareOptions["whiteSpace"]
}

/** One visible virtualized row produced by the shared Pretext paragraph list. */
export type VirtualizedPretextParagraphRow<Item, RowData> = {
  key: string
  item: Item
  index: number
  top: number
  height: number
  paragraph: PretextParagraphMeasurement
  data: RowData
}

/** Props accepted by the shared virtualized Pretext paragraph list. */
export type VirtualizedPretextParagraphListProps<Item, RowData> = {
  items: readonly Item[]
  overscanPx?: number
  initialScrollPosition?: "top" | "bottom"
  scrollCacheKey?: string
  loadingFallback?: ComponentChild
  getParagraphSpec: (item: Item, index: number, viewportWidth: number) => PretextParagraphSpec
  layoutRow: (input: {
    item: Item
    index: number
    top: number
    viewportWidth: number
    paragraph: PretextParagraphMeasurement
  }) => {
    height: number
    data: RowData
  }
  renderRow: (row: VirtualizedPretextParagraphRow<Item, RowData>) => ComponentChild
}

/** Returns the cached prepared representation for one paragraph. */
function prepareParagraph(spec: PretextParagraphSpec): PreparedTextWithSegments {
  const cacheKey = `${spec.font}::${spec.whiteSpace ?? "normal"}::${spec.text}`
  const cachedPrepared = preparedParagraphCache.get(cacheKey)

  if (cachedPrepared) {
    return cachedPrepared
  }

  const prepared = prepareWithSegments(spec.text, spec.font, {
    whiteSpace: spec.whiteSpace,
  })
  preparedParagraphCache.set(cacheKey, prepared)
  return prepared
}

/** Measures one paragraph with Pretext for the current max width. */
function measureParagraph(spec: PretextParagraphSpec): PretextParagraphMeasurement {
  const prepared = prepareParagraph(spec)
  const lineLayout = layoutWithLines(prepared, spec.maxWidth, spec.lineHeight)
  const maxLineWidth = lineLayout.lines.reduce((widest, line) => Math.max(widest, line.width), 0)

  return {
    lines: lineLayout.lines,
    lineCount: lineLayout.lineCount,
    height: lineLayout.height,
    maxLineWidth,
  }
}

/** Finds the first row whose lower bound intersects the requested vertical offset. */
function findStartIndex<RowData>(
  rows: readonly VirtualizedPretextParagraphRow<unknown, RowData>[],
  scrollOffset: number,
): number {
  let low = 0
  let high = rows.length - 1
  let result = rows.length

  while (low <= high) {
    const middle = Math.floor((low + high) / 2)
    const row = rows[middle]

    if (row.top + row.height >= scrollOffset) {
      result = middle
      high = middle - 1
    } else {
      low = middle + 1
    }
  }

  return Math.min(result, rows.length)
}

/** Finds the first row that starts after the requested vertical offset. */
function findEndIndex<RowData>(
  rows: readonly VirtualizedPretextParagraphRow<unknown, RowData>[],
  scrollOffset: number,
): number {
  let low = 0
  let high = rows.length - 1
  let result = rows.length

  while (low <= high) {
    const middle = Math.floor((low + high) / 2)
    const row = rows[middle]

    if (row.top > scrollOffset) {
      result = middle
      high = middle - 1
    } else {
      low = middle + 1
    }
  }

  return Math.min(result, rows.length)
}

/** Renders a virtualized list of Pretext-measured paragraphs inside the nearest viewport provider. */
export function VirtualizedPretextParagraphList<Item, RowData>(
  props: VirtualizedPretextParagraphListProps<Item, RowData>,
) {
  const viewport = usePretextViewport()
  const hasRestoredScrollRef = useRef(false)
  const effectiveInitialScroll = props.initialScrollPosition ?? "top"
  const overscanPx = props.overscanPx ?? 320

  useEffect(() => {
    hasRestoredScrollRef.current = false
  }, [effectiveInitialScroll, props.items, props.scrollCacheKey])

  useEffect(() => {
    return () => {
      const viewportElement = viewport.viewportRef.current

      if (!viewportElement || !props.scrollCacheKey) {
        return
      }

      paragraphScrollCache.set(props.scrollCacheKey, viewportElement.scrollTop)
    }
  }, [props.scrollCacheKey, viewport.viewportRef])

  const layout = useMemo(() => {
    if (!viewport.fontsReady || viewport.width <= 0) {
      return {
        rows: [] as VirtualizedPretextParagraphRow<Item, RowData>[],
        totalHeight: 0,
      }
    }

    let nextTop = 0

    const rows = props.items.map((item, index) => {
      const paragraphSpec = props.getParagraphSpec(item, index, viewport.width)
      const paragraph = measureParagraph(paragraphSpec)
      const rowLayout = props.layoutRow({
        item,
        index,
        top: nextTop,
        viewportWidth: viewport.width,
        paragraph,
      })
      const row: VirtualizedPretextParagraphRow<Item, RowData> = {
        key: paragraphSpec.key,
        item,
        index,
        top: nextTop,
        height: rowLayout.height,
        paragraph,
        data: rowLayout.data,
      }

      nextTop += row.height
      return row
    })

    return {
      rows,
      totalHeight: nextTop,
    }
  }, [props, viewport.fontsReady, viewport.width])

  const visibleRows = useMemo(() => {
    if (layout.rows.length === 0) {
      return []
    }

    const visibleTop = Math.max(0, viewport.scrollTop - overscanPx)
    const visibleBottom = viewport.scrollTop + viewport.height + overscanPx
    const startIndex = findStartIndex(
      layout.rows as readonly VirtualizedPretextParagraphRow<unknown, RowData>[],
      visibleTop,
    )
    const endIndex = findEndIndex(
      layout.rows as readonly VirtualizedPretextParagraphRow<unknown, RowData>[],
      visibleBottom,
    )

    return layout.rows.slice(startIndex, Math.max(startIndex + 1, endIndex))
  }, [layout.rows, overscanPx, viewport.height, viewport.scrollTop])

  useLayoutEffect(() => {
    const viewportElement = viewport.viewportRef.current

    if (!viewportElement || !viewport.fontsReady || viewport.height <= 0) {
      return
    }

    const cachedScrollTop = props.scrollCacheKey
      ? paragraphScrollCache.get(props.scrollCacheKey)
      : undefined

    if (!hasRestoredScrollRef.current && typeof cachedScrollTop === "number") {
      viewportElement.scrollTop = cachedScrollTop
      hasRestoredScrollRef.current = true
      return
    }

    if (!hasRestoredScrollRef.current && effectiveInitialScroll === "bottom") {
      viewportElement.scrollTop = Math.max(0, layout.totalHeight - viewport.height)
      hasRestoredScrollRef.current = true
      return
    }

    if (!hasRestoredScrollRef.current && effectiveInitialScroll === "top") {
      viewportElement.scrollTop = 0
      hasRestoredScrollRef.current = true
    }
  }, [
    effectiveInitialScroll,
    layout.totalHeight,
    props.scrollCacheKey,
    viewport.fontsReady,
    viewport.height,
    viewport.viewportRef,
  ])

  return (
    <>
      {viewport.fontsReady && viewport.width > 0 ? (
        <div class={listCanvasClass} style={{ height: `${layout.totalHeight}px` }}>
          {visibleRows.map((row) => props.renderRow(row))}
        </div>
      ) : null}
      {!viewport.fontsReady || viewport.width <= 0
        ? (props.loadingFallback ?? (
            <div class={loadingOverlayClass}>Preparing paragraph layout...</div>
          ))
        : null}
    </>
  )
}
