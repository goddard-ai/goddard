import { Search } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { openProjectFromFilesystem, openProjectTab } from "./actions.ts"
import { orderProjectsByRecentActivity } from "./project-context.ts"
import type { ProjectRecord } from "./project-registry.ts"
import { lookupProject } from "./project-registry.ts"
import styles from "./switch-project-dropdown.style.ts"

type SwitchProjectItem = {
  id: "open-folder";
  kind: "open-folder";
} | {
  id: string;
  kind: "project";
  project: ProjectRecord;
}

function projectMatchesSearch(project: ProjectRecord, search: string) {
  const normalizedSearch = search.trim().toLowerCase()

  if (normalizedSearch.length === 0) {
    return true
  }

  return project.name.toLowerCase().includes(normalizedSearch) ||
    project.path.toLowerCase().includes(normalizedSearch)
}

/** Renders the header-anchored searchable project switcher dropdown. */
export component SwitchProjectDropdown() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const contentRef = useRef<HTMLDivElement | null>(null)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})

  const [isOpen, setIsOpen] = useState(false)
  const [search, setSearch] = useState("")
  const [highlightedIndex, setHighlightedIndex] = useState(0)

  const projects = orderProjectsByRecentActivity(
    projectRegistry.projectList,
    projectContext.recentProjectPaths,
  )
  const activeProject =
    projectContext.activeProjectPath === null
      ? null
      : lookupProject(projectRegistry, projectContext.activeProjectPath)

  const filteredProjects = projects.filter((project) => projectMatchesSearch(project, search))
  const items: SwitchProjectItem[] =
    search.trim().length === 0
      ? [
          { id: "open-folder", kind: "open-folder" },
          ...filteredProjects.map(
            (project) => ({
              id: project.path,
              kind: "project" as const,
              project,
            }),
          ),
        ]
      : filteredProjects.map(
          (project) => ({
            id: project.path,
            kind: "project" as const,
            project,
          }),
        )
  const highlightedItemIndex =
    items.length === 0 ? 0 : Math.min(highlightedIndex, items.length - 1)
  const highlightedItem = items[highlightedItemIndex]
  const highlightedItemId = highlightedItem?.id ?? null

  async function openFolderFromSwitcher() {
    setIsOpen(false)
    await openProjectFromFilesystem(
      {
        projectContext,
        projectRegistry,
        workbenchTabSet,
      },
    )
  }

  function moveHighlightedIndex(delta: -1 | 1) {
    setHighlightedIndex((currentIndex) => {
      if (items.length === 0) {
        return 0
      }

      const currentHighlightedIndex = Math.min(currentIndex, items.length - 1)

      return delta > 0
        ? Math.min(currentHighlightedIndex + 1, items.length - 1)
        : Math.max(currentHighlightedIndex - 1, 0)
    })
  }

  function activateItem(item: SwitchProjectItem | undefined) {
    if (!item) {
      return
    }

    if (item.kind === "open-folder") {
      void openFolderFromSwitcher()
      return
    }

    setIsOpen(false)
    openProjectTab(
      {
        projectContext,
        projectPath: item.project.path,
        projectRegistry,
        workbenchTabSet,
      },
    )
  }

  useAppCommand(AppCommand.navigation.openSwitchProject, () => {
    setIsOpen(true)
  })

  useAppCommand(AppCommand.projects.openFolder, async () => {
    await openFolderFromSwitcher()
  })

  useEffect(() => {
    if (!isOpen) {
      setSearch("")
      setHighlightedIndex(0)
      return
    }

    queueMicrotask(() => {
      const input = inputRef.current
      input?.focus()
      input?.setSelectionRange(input.value.length, input.value.length)
    })
  }, [isOpen])

  useEffect(() => {
    if (!isOpen || !highlightedItemId) {
      return
    }

    itemRefs.current[highlightedItemId]?.scrollIntoView(
      {
        block: "nearest",
      },
    )
  }, [highlightedItemId, isOpen])

  useListener(document, "mousedown", (event) => {
    if (!isOpen) {
      return
    }

    const target = event.target

    if (!(target instanceof Node)) {
      return
    }

    if (contentRef.current?.contains(target) || triggerRef.current?.contains(target)) {
      return
    }

    setIsOpen(false)
  })

  useListener(document, "keydown", (event) => {
    if (!isOpen || event.key !== "Escape") {
      return
    }

    event.preventDefault()
    setIsOpen(false)
    queueMicrotask(() => {
      triggerRef.current?.focus()
    })
  })

  <button
    ref={triggerRef}
    aria-expanded={isOpen ? "true" : "false"}
    aria-haspopup="dialog"
    aria-label="Switch project"
    class={styles.trigger}
    type="button"
    onClick={() => {
      setIsOpen(!isOpen)
    }}
    onKeyDown={(event) => {
      if (event.key === "ArrowDown" && !isOpen) {
        event.preventDefault()
        setIsOpen(true)
      }
    }}
  >
    <span class={styles.triggerLabel}>
      {activeProject?.name ?? "Open project"}
    </span>
  </button>
  {isOpen
    ? <tsx>
        <div ref={contentRef} class={styles.content}>
          <label class={styles.searchField}>
            <span class={styles.srOnly}>
              {"Search projects"}
            </span>
            <Search aria-hidden={true} size={16} strokeWidth={2.1} />
            <input ref={inputRef} class={styles.searchInput} placeholder="Search projects" value={search} onInput={(
              event,
            ) => {
              setSearch(event.currentTarget.value)
              setHighlightedIndex(0)
            }} onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault()
                moveHighlightedIndex(1)
                return
              }

              if (event.key === "ArrowUp") {
                event.preventDefault()
                moveHighlightedIndex(-1)
                return
              }

              if (event.key === "Enter") {
                event.preventDefault()
                activateItem(highlightedItem)
              }
            }} />
          </label>
          {items.length > 0
            ? <tsx>
                <ul class={styles.list}>
                  for (const item of items; index index)
                  {<li key={item.id}>
                    <button ref={(element) => {
                      itemRefs.current[item.id] = element
                    }} class={styles.item} data-highlighted={highlightedItemIndex === index
                      ? "true"
                      : "false"} type="button" onMouseEnter={() => {
                      setHighlightedIndex(index)
                    }} onClick={() => {
                      activateItem(item)
                    }}>
                      <span class={styles.itemBody}>
                        {item.kind === "open-folder"
                          ? <tsx>
                              <span class={styles.itemTitle}>
                                {"Open folder…"}
                              </span>
                              <span class={styles.itemDetail}>
                                {"Add a local project from your filesystem."}
                              </span>
                            </tsx>
                          : <tsx>
                              <span class={styles.itemTitleEllipsis}>
                                {item.project.name}
                              </span>
                              <span class={styles.itemDetailEllipsis}>
                                {item.project.path}
                              </span>
                            </tsx>}
                      </span>
                      {item.kind === "project" && activeProject?.path === item.project.path
                        ? <tsx>
                            <span class={styles.activeBadge}>
                              {"Active"}
                            </span>
                          </tsx>
                        : null}
                    </button>
                  </li>}
                </ul>
              </tsx>
            : <tsx>
                <p class={styles.empty}>
                  {"No matching projects."}
                </p>
              </tsx>}
        </div>
      </tsx>
    : null}
}

export default SwitchProjectDropdown
