import { useLayoutEffect, useMemo, useRef } from "preact/hooks"

/** Imperative controller for a DOM-backed indexed list navigation surface. */
export type ListNavigationController = {
  /** Returns the current activation target after clamping and disabled-row skipping. */
  activeIndex: () => number
  /** Moves DOM focus to the currently active registered item, if it is mounted. */
  focusActiveItem: () => void
  /** Moves DOM focus to a registered item by index, if it is mounted. */
  focusItem: (index: number) => void
  /**
   * Registers the DOM element for an item index.
   *
   * The ref function supports both callback-ref cleanup returns and later `null`
   * calls so callers can use whichever lifecycle their renderer provides.
   */
  itemRef: (index: number) => (element: HTMLElement | null) => void | (() => void)
  /** Moves the active index by one step, respecting wrapping and disabled rows. */
  moveActiveIndex: (delta: -1 | 1) => void
  /**
   * Handles ArrowUp, ArrowDown, Home, End, and Enter for the list.
   *
   * Returns `true` when it handled the event and prevented the default action.
   */
  onKeyDown: (event: KeyboardEvent) => boolean
  /** Resets the activation target to the first enabled row. */
  resetActiveIndex: () => void
  /** Sets the activation target, clamping to the current count and skipping disabled rows. */
  setActiveIndex: (index: number) => void
}

/** Configuration for `useListNavigation`. */
export type ListNavigationOptions = {
  /**
   * DOM attribute used to mark active rows.
   *
   * Defaults to `data-highlighted`; the active row receives `"true"` and other
   * registered rows receive `"false"`. Touch-capable devices always receive
   * `"false"` so activation targets do not appear selected there.
   */
  activeAttribute?: string
  /**
   * Returns the number of visible rows the controller should navigate.
   *
   * The hook intentionally uses a count instead of an item array so callers keep
   * filtering, ranking, identity, and rendering policy outside the primitive.
   */
  count: () => number
  /** Called with the active index when Enter activates an enabled row. */
  onActivate?: (index: number) => void
  /** Called after the primitive changes the active index. */
  onActiveIndexChange?: (index: number) => void
  /**
   * Controls whether active rows scroll into view.
   *
   * Defaults to `{ block: "nearest" }`; pass `false` to disable scrolling.
   */
  scrollIntoView?: boolean | ScrollIntoViewOptions
  /**
   * Suppresses pointer-enter highlighting while it returns true.
   *
   * Use this for menus that temporarily ignore hover after keyboard or typeahead
   * movement until the pointer actually moves.
   */
  shouldIgnorePointer?: () => boolean
  /** Whether keyboard movement wraps at list boundaries. Defaults to true. */
  wrap?: boolean
}

/** `useListNavigation` controller plus a ref for wiring a search input element. */
export type SearchNavigationController = ListNavigationController & {
  /**
   * Registers a search input and wires input, Escape, and list-navigation keys.
   *
   * Non-search lists may ignore this ref and still use the controller as a list
   * navigation controller.
   */
  inputRef: (element: HTMLInputElement | null) => void | (() => void)
}

/** Configuration for `useSearchNavigation`. */
export type SearchNavigationOptions = ListNavigationOptions & {
  /** Called when Escape is pressed in the registered search input. */
  onEscape?: () => void
  /** Called with the input value on each input event before the active row is reset. */
  onQueryChange: (query: string) => void
}

const defaultActiveAttribute = "data-highlighted"
const defaultScrollOptions: ScrollIntoViewOptions = { block: "nearest" }
type ActiveIndexOrigin = "navigation" | "pointer"

// Browsers expose touch capability, but no reliable equivalent for keyboard support.
function shouldExposeActiveIndex() {
  return typeof navigator === "undefined" || !navigator.maxTouchPoints
}

/**
 * Manages DOM-backed active-row state for indexed list surfaces.
 *
 * The hook owns keyboard movement, pointer-enter highlighting, disabled-row
 * inference from registered DOM elements, active DOM attributes, and
 * scroll-into-view. Callers keep item data, filtering, rendering, and ARIA roles
 * outside the primitive.
 */
export function useListNavigation(options: ListNavigationOptions): ListNavigationController {
  const optionsRef = useRef(options)
  const activeIndexRef = useRef(0)
  const itemElementsRef = useRef(new Map<number, HTMLElement>())
  const itemCleanupRef = useRef(new Map<number, () => void>())
  const ignorePointerHighlightRef = useRef(true)
  const lastPointerPositionRef = useRef<{ x: number; y: number } | null>(null)
  const activeIndexOriginRef = useRef<ActiveIndexOrigin>("navigation")
  const syncActiveElementRef = useRef(() => {})

  optionsRef.current = options

  const controller = useMemo<ListNavigationController>(() => {
    function getCount() {
      return Math.max(0, optionsRef.current.count())
    }

    function getActiveAttribute() {
      return optionsRef.current.activeAttribute ?? defaultActiveAttribute
    }

    function getScrollOptions() {
      const scrollIntoView = optionsRef.current.scrollIntoView

      if (scrollIntoView === false) {
        return null
      }

      return scrollIntoView === true || scrollIntoView === undefined
        ? defaultScrollOptions
        : scrollIntoView
    }

    function isItemDisabled(index: number) {
      const element = itemElementsRef.current.get(index)

      return element?.matches(":disabled, [disabled], [aria-disabled='true']") ?? false
    }

    function findEnabledIndex(startIndex: number, direction: -1 | 1) {
      const count = getCount()

      if (count === 0) {
        return 0
      }

      const wrap = optionsRef.current.wrap ?? true

      for (let offset = 0; offset < count; offset += 1) {
        const nextIndex = startIndex + direction * offset

        if (!wrap && (nextIndex < 0 || nextIndex >= count)) {
          return activeIndexRef.current
        }

        const wrappedIndex = (nextIndex + count) % count

        if (!isItemDisabled(wrappedIndex)) {
          return wrappedIndex
        }
      }

      return activeIndexRef.current
    }

    function suppressPointerHighlightUntilMove() {
      ignorePointerHighlightRef.current = true
    }

    function updatePointerPosition(event: PointerEvent) {
      const previousPosition = lastPointerPositionRef.current
      const nextPosition = {
        x: event.clientX,
        y: event.clientY,
      }
      const moved =
        !previousPosition ||
        previousPosition.x !== nextPosition.x ||
        previousPosition.y !== nextPosition.y

      if (moved) {
        ignorePointerHighlightRef.current = false
      }

      lastPointerPositionRef.current = nextPosition
      return moved
    }

    function shouldIgnorePointerHighlight() {
      return ignorePointerHighlightRef.current || optionsRef.current.shouldIgnorePointer?.()
    }

    function syncActiveElement(options?: { allowScroll?: boolean; previousIndex?: number }) {
      const previousIndex = options?.previousIndex ?? activeIndexRef.current
      const count = getCount()
      const activeAttribute = getActiveAttribute()

      if (count === 0) {
        activeIndexRef.current = 0
      } else if (activeIndexRef.current >= count) {
        activeIndexRef.current = findEnabledIndex(count - 1, -1)
      } else if (activeIndexRef.current < 0) {
        activeIndexRef.current = findEnabledIndex(0, 1)
      } else if (isItemDisabled(activeIndexRef.current)) {
        activeIndexRef.current = findEnabledIndex(activeIndexRef.current, 1)
      }

      for (const [index, element] of itemElementsRef.current) {
        if (index >= count) {
          element.setAttribute(activeAttribute, "false")
          continue
        }

        if (
          shouldExposeActiveIndex() &&
          index === activeIndexRef.current &&
          !isItemDisabled(index)
        ) {
          element.setAttribute(activeAttribute, "true")
          if (options?.allowScroll && activeIndexOriginRef.current === "navigation") {
            suppressPointerHighlightUntilMove()
            const scrollOptions = getScrollOptions()

            if (scrollOptions) {
              element.scrollIntoView(scrollOptions)
            }
          }
        } else {
          element.setAttribute(activeAttribute, "false")
        }
      }

      if (activeIndexRef.current !== previousIndex) {
        optionsRef.current.onActiveIndexChange?.(activeIndexRef.current)
      }
    }

    syncActiveElementRef.current = syncActiveElement

    function setActiveIndexWithDirection(index: number, direction: -1 | 1) {
      const count = getCount()
      const previousIndex = activeIndexRef.current

      if (count === 0) {
        activeIndexRef.current = 0
        activeIndexOriginRef.current = "navigation"
        syncActiveElement({ allowScroll: true, previousIndex })
        return
      }

      const clampedIndex = Math.min(Math.max(index, 0), count - 1)

      activeIndexRef.current = isItemDisabled(clampedIndex)
        ? findEnabledIndex(clampedIndex, direction)
        : clampedIndex
      activeIndexOriginRef.current = "navigation"
      suppressPointerHighlightUntilMove()
      syncActiveElement({ allowScroll: true, previousIndex })
    }

    function setActiveIndex(index: number) {
      setActiveIndexWithDirection(index, 1)
    }

    function setActiveIndexFromPointer(index: number) {
      const count = getCount()
      const previousIndex = activeIndexRef.current

      if (count === 0) {
        activeIndexRef.current = 0
        activeIndexOriginRef.current = "pointer"
        syncActiveElement({ previousIndex })
        return
      }

      const clampedIndex = Math.min(Math.max(index, 0), count - 1)

      activeIndexRef.current = isItemDisabled(clampedIndex)
        ? findEnabledIndex(clampedIndex, 1)
        : clampedIndex
      activeIndexOriginRef.current = "pointer"
      syncActiveElement({ previousIndex })
    }

    function moveActiveIndex(delta: -1 | 1) {
      const count = getCount()
      const previousIndex = activeIndexRef.current

      if (count === 0) {
        activeIndexRef.current = 0
        activeIndexOriginRef.current = "navigation"
        syncActiveElement({ allowScroll: true, previousIndex })
        return
      }

      const nextIndex = findEnabledIndex(activeIndexRef.current + delta, delta)

      activeIndexRef.current = nextIndex
      activeIndexOriginRef.current = "navigation"
      suppressPointerHighlightUntilMove()
      syncActiveElement({ allowScroll: true, previousIndex })
    }

    function activateActiveIndex() {
      const count = getCount()

      if (count === 0 || isItemDisabled(activeIndexRef.current)) {
        return
      }

      optionsRef.current.onActivate?.(activeIndexRef.current)
    }

    function focusItem(index: number) {
      itemElementsRef.current.get(index)?.focus()
    }

    function onKeyDown(event: KeyboardEvent) {
      switch (event.key) {
        case "ArrowDown":
          event.preventDefault()
          moveActiveIndex(1)
          return true
        case "ArrowUp":
          event.preventDefault()
          moveActiveIndex(-1)
          return true
        case "Home":
          event.preventDefault()
          setActiveIndexWithDirection(0, 1)
          return true
        case "End":
          event.preventDefault()
          setActiveIndexWithDirection(getCount() - 1, -1)
          return true
        case "Enter":
          event.preventDefault()
          activateActiveIndex()
          return true
        default:
          return false
      }
    }

    function registerItem(index: number, element: HTMLElement | null) {
      itemCleanupRef.current.get(index)?.()
      itemCleanupRef.current.delete(index)

      if (!element) {
        itemElementsRef.current.delete(index)
        return
      }

      itemElementsRef.current.set(index, element)
      suppressPointerHighlightUntilMove()

      const handlePointerEnter = () => {
        if (!shouldIgnorePointerHighlight() && !isItemDisabled(index)) {
          setActiveIndexFromPointer(index)
        }
      }
      const handlePointerMove = (event: PointerEvent) => {
        if (
          updatePointerPosition(event) &&
          !shouldIgnorePointerHighlight() &&
          !isItemDisabled(index)
        ) {
          setActiveIndexFromPointer(index)
        }
      }

      element.addEventListener("pointerenter", handlePointerEnter)
      element.addEventListener("pointermove", handlePointerMove)
      itemCleanupRef.current.set(index, () => {
        element.removeEventListener("pointerenter", handlePointerEnter)
        element.removeEventListener("pointermove", handlePointerMove)
        itemElementsRef.current.delete(index)
      })
      syncActiveElement()
    }

    return {
      activeIndex() {
        return activeIndexRef.current
      },
      focusActiveItem() {
        focusItem(activeIndexRef.current)
      },
      focusItem,
      itemRef(index) {
        return (element) => {
          registerItem(index, element)

          if (!element) {
            return
          }

          return () => {
            registerItem(index, null)
          }
        }
      },
      moveActiveIndex,
      onKeyDown,
      resetActiveIndex() {
        setActiveIndexWithDirection(0, 1)
      },
      setActiveIndex,
    }
  }, [])

  useLayoutEffect(() => {
    // Registration and render-driven reconciliation must not replay scrolling
    // for an active index that came from pointer hover during manual scroll.
    syncActiveElementRef.current()
  })

  useLayoutEffect(() => {
    return () => {
      for (const cleanup of itemCleanupRef.current.values()) {
        cleanup()
      }

      itemCleanupRef.current.clear()
      itemElementsRef.current.clear()
    }
  }, [])

  useLayoutEffect(() => {
    const handleScroll = () => {
      ignorePointerHighlightRef.current = true
    }

    document.addEventListener("scroll", handleScroll, true)
    return () => {
      document.removeEventListener("scroll", handleScroll, true)
    }
  }, [])

  return controller
}

/**
 * Wires a search input element to `useListNavigation`.
 *
 * The input ref listens for query changes, resets the active row after input,
 * delegates list-navigation keys, and optionally handles Escape. Filtering and
 * ranking stay with the caller.
 */
export function useSearchNavigation(options: SearchNavigationOptions): SearchNavigationController {
  const optionsRef = useRef(options)
  const inputCleanupRef = useRef<(() => void) | null>(null)
  const listNavigation = useListNavigation(options)

  optionsRef.current = options

  return useMemo<SearchNavigationController>(
    () => ({
      activeIndex: listNavigation.activeIndex,
      focusActiveItem: listNavigation.focusActiveItem,
      focusItem: listNavigation.focusItem,
      inputRef(element) {
        inputCleanupRef.current?.()
        inputCleanupRef.current = null

        if (!element) {
          return
        }

        const handleInput = () => {
          optionsRef.current.onQueryChange(element.value)
          listNavigation.resetActiveIndex()
        }
        const handleKeyDown = (event: KeyboardEvent) => {
          if (event.key === "Escape" && optionsRef.current.onEscape) {
            event.preventDefault()
            event.stopPropagation()
            optionsRef.current.onEscape()
            return
          }

          listNavigation.onKeyDown(event)
        }

        element.addEventListener("input", handleInput)
        element.addEventListener("keydown", handleKeyDown)
        inputCleanupRef.current = () => {
          element.removeEventListener("input", handleInput)
          element.removeEventListener("keydown", handleKeyDown)
        }

        return () => {
          inputCleanupRef.current?.()
          inputCleanupRef.current = null
        }
      },
      itemRef: listNavigation.itemRef,
      moveActiveIndex: listNavigation.moveActiveIndex,
      onKeyDown: listNavigation.onKeyDown,
      resetActiveIndex: listNavigation.resetActiveIndex,
      setActiveIndex: listNavigation.setActiveIndex,
    }),
    [listNavigation],
  )
}
