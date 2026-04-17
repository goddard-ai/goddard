import { Popover } from "@ark-ui/react/popover"
import { Search } from "lucide-react"
import { useEffect, useRef, useState } from "preact/hooks"

import type { ProjectRecord } from "./project-registry.ts"
import styles from "./switch-project-dropdown.style.ts"

type SwitchProjectItem =
  | {
      id: "open-folder"
      kind: "open-folder"
    }
  | {
      id: string
      kind: "project"
      project: ProjectRecord
    }

function projectMatchesSearch(project: ProjectRecord, search: string) {
  const normalizedSearch = search.trim().toLowerCase()

  if (normalizedSearch.length === 0) {
    return true
  }

  return (
    project.name.toLowerCase().includes(normalizedSearch) ||
    project.path.toLowerCase().includes(normalizedSearch)
  )
}

/** Renders the header-anchored searchable project switcher dropdown. */
export function SwitchProjectDropdown(props: {
  activeProjectLabel: string
  activeProjectPath: string | null
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  onOpenFolder: () => Promise<void> | void
  onSelectProject: (path: string) => void
  projects: readonly ProjectRecord[]
}) {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const [search, setSearch] = useState("")
  const [highlightedIndex, setHighlightedIndex] = useState(0)

  const filteredProjects = props.projects.filter((project) => projectMatchesSearch(project, search))
  const items: SwitchProjectItem[] =
    search.trim().length === 0
      ? [
          { id: "open-folder", kind: "open-folder" },
          ...filteredProjects.map((project) => ({
            id: project.path,
            kind: "project" as const,
            project,
          })),
        ]
      : filteredProjects.map((project) => ({
          id: project.path,
          kind: "project" as const,
          project,
        }))
  const highlightedItemId = items[highlightedIndex]?.id ?? null

  useEffect(() => {
    if (!props.isOpen) {
      setSearch("")
      setHighlightedIndex(0)
    }
  }, [props.isOpen])

  useEffect(() => {
    setHighlightedIndex((currentIndex) => {
      if (items.length === 0) {
        return 0
      }

      return Math.min(currentIndex, items.length - 1)
    })
  }, [items.length])

  useEffect(() => {
    if (!props.isOpen || !highlightedItemId) {
      return
    }

    itemRefs.current[highlightedItemId]?.scrollIntoView({
      block: "nearest",
    })
  }, [highlightedItemId, props.isOpen])

  function activateItem(item: SwitchProjectItem | undefined) {
    if (!item) {
      return
    }

    props.onOpenChange(false)

    if (item.kind === "open-folder") {
      void props.onOpenFolder()
      return
    }

    props.onSelectProject(item.project.path)
  }

  return (
    <Popover.Root
      closeOnInteractOutside={true}
      initialFocusEl={() => inputRef.current}
      lazyMount={true}
      open={props.isOpen}
      portalled={false}
      positioning={{ placement: "bottom", sameWidth: true }}
      unmountOnExit={true}
      onOpenChange={(details) => {
        props.onOpenChange(details.open)
      }}
    >
      <Popover.Trigger asChild>
        <button aria-label="Switch project" class={styles.trigger} type="button">
          <span class={styles.triggerLabel}>{props.activeProjectLabel}</span>
        </button>
      </Popover.Trigger>

      <Popover.Positioner>
        <Popover.Content class={styles.content}>
          <label class={styles.searchField}>
            <span class={styles.srOnly}>Search projects</span>
            <Search aria-hidden={true} size={16} strokeWidth={2.1} />
            <input
              ref={inputRef}
              class={styles.searchInput}
              placeholder="Search projects"
              value={search}
              onInput={(event) => {
                setSearch(event.currentTarget.value)
                setHighlightedIndex(0)
              }}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown") {
                  event.preventDefault()
                  setHighlightedIndex((currentIndex) =>
                    items.length === 0 ? 0 : Math.min(currentIndex + 1, items.length - 1),
                  )
                  return
                }

                if (event.key === "ArrowUp") {
                  event.preventDefault()
                  setHighlightedIndex((currentIndex) => Math.max(currentIndex - 1, 0))
                  return
                }

                if (event.key === "Enter") {
                  event.preventDefault()
                  activateItem(items[highlightedIndex])
                  return
                }
              }}
            />
          </label>

          {items.length > 0 ? (
            <ul class={styles.list}>
              {items.map((item, index) => {
                const isHighlighted = highlightedIndex === index

                if (item.kind === "open-folder") {
                  return (
                    <li key={item.id}>
                      <button
                        ref={(element) => {
                          itemRefs.current[item.id] = element
                        }}
                        class={styles.item}
                        data-highlighted={isHighlighted ? "true" : "false"}
                        type="button"
                        onMouseEnter={() => {
                          setHighlightedIndex(index)
                        }}
                        onClick={() => {
                          activateItem(item)
                        }}
                      >
                        <span class={styles.itemBody}>
                          <span class={styles.itemTitle}>Open folder…</span>
                          <span class={styles.itemDetail}>
                            Add a local project from your filesystem.
                          </span>
                        </span>
                      </button>
                    </li>
                  )
                }

                const isActiveProject = props.activeProjectPath === item.project.path

                return (
                  <li key={item.id}>
                    <button
                      ref={(element) => {
                        itemRefs.current[item.id] = element
                      }}
                      class={styles.item}
                      data-highlighted={isHighlighted ? "true" : "false"}
                      type="button"
                      onMouseEnter={() => {
                        setHighlightedIndex(index)
                      }}
                      onClick={() => {
                        activateItem(item)
                      }}
                    >
                      <span class={styles.itemBody}>
                        <span class={styles.itemTitleEllipsis}>{item.project.name}</span>
                        <span class={styles.itemDetailEllipsis}>{item.project.path}</span>
                      </span>

                      {isActiveProject ? <span class={styles.activeBadge}>Active</span> : null}
                    </button>
                  </li>
                )
              })}
            </ul>
          ) : (
            <p class={styles.empty}>No matching projects.</p>
          )}
        </Popover.Content>
      </Popover.Positioner>
    </Popover.Root>
  )
}

export default SwitchProjectDropdown
