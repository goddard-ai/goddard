import { useLayoutEffect, useMemo, useRef } from "preact/hooks"

export type ListNavigationController = {
  activeIndex: () => number
  itemRef: (index: number) => (element: HTMLElement | null) => void | (() => void)
  moveActiveIndex: (delta: -1 | 1) => void
  onKeyDown: (event: KeyboardEvent) => boolean
  resetActiveIndex: () => void
  setActiveIndex: (index: number) => void
}

export type ListNavigationOptions = {
  activeAttribute?: string
  count: () => number
  onActivate?: (index: number) => void
  scrollIntoView?: boolean | ScrollIntoViewOptions
  wrap?: boolean
}

export type SearchNavigationController = Omit<ListNavigationController, "onKeyDown"> & {
  inputRef: (element: HTMLInputElement | null) => void | (() => void)
}

export type SearchNavigationOptions = ListNavigationOptions & {
  onEscape?: () => void
  onQueryChange: (query: string) => void
}

const defaultActiveAttribute = "data-highlighted"
const defaultScrollOptions: ScrollIntoViewOptions = { block: "nearest" }

/** Manages DOM-backed active-row state for indexed list surfaces. */
export function useListNavigation(options: ListNavigationOptions): ListNavigationController {
  const optionsRef = useRef(options)
  const activeIndexRef = useRef(0)
  const itemElementsRef = useRef(new Map<number, HTMLElement>())
  const itemCleanupRef = useRef(new Map<number, () => void>())

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

    function syncActiveElement() {
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

        if (index === activeIndexRef.current && !isItemDisabled(index)) {
          element.setAttribute(activeAttribute, "true")
          const scrollOptions = getScrollOptions()

          if (scrollOptions) {
            element.scrollIntoView(scrollOptions)
          }
        } else {
          element.setAttribute(activeAttribute, "false")
        }
      }
    }

    function setActiveIndexWithDirection(index: number, direction: -1 | 1) {
      const count = getCount()

      if (count === 0) {
        activeIndexRef.current = 0
        syncActiveElement()
        return
      }

      const clampedIndex = Math.min(Math.max(index, 0), count - 1)

      activeIndexRef.current = isItemDisabled(clampedIndex)
        ? findEnabledIndex(clampedIndex, direction)
        : clampedIndex
      syncActiveElement()
    }

    function setActiveIndex(index: number) {
      setActiveIndexWithDirection(index, 1)
    }

    function moveActiveIndex(delta: -1 | 1) {
      const count = getCount()

      if (count === 0) {
        activeIndexRef.current = 0
        syncActiveElement()
        return
      }

      const nextIndex = findEnabledIndex(activeIndexRef.current + delta, delta)

      activeIndexRef.current = nextIndex
      syncActiveElement()
    }

    function activateActiveIndex() {
      const count = getCount()

      if (count === 0 || isItemDisabled(activeIndexRef.current)) {
        return
      }

      optionsRef.current.onActivate?.(activeIndexRef.current)
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

      const handlePointerEnter = () => {
        if (!isItemDisabled(index)) {
          setActiveIndex(index)
        }
      }

      element.addEventListener("pointerenter", handlePointerEnter)
      itemCleanupRef.current.set(index, () => {
        element.removeEventListener("pointerenter", handlePointerEnter)
        itemElementsRef.current.delete(index)
      })
      syncActiveElement()
    }

    return {
      activeIndex() {
        return activeIndexRef.current
      },
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
    controller.setActiveIndex(controller.activeIndex())
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

  return controller
}

/** Wires an input element to list navigation for search-driven result lists. */
export function useSearchNavigation(options: SearchNavigationOptions): SearchNavigationController {
  const optionsRef = useRef(options)
  const inputCleanupRef = useRef<(() => void) | null>(null)
  const listNavigation = useListNavigation(options)

  optionsRef.current = options

  return useMemo<SearchNavigationController>(
    () => ({
      activeIndex: listNavigation.activeIndex,
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
      resetActiveIndex: listNavigation.resetActiveIndex,
      setActiveIndex: listNavigation.setActiveIndex,
    }),
    [listNavigation],
  )
}
