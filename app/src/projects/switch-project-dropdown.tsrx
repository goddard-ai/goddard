import { concat } from "radashi"
import { Search } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { openProjectFromFilesystem, openProjectTab } from "./actions.ts"
import { orderProjectsByRecentActivity } from "./project-context.ts"
import type { ProjectRecord } from "./project-registry.ts"
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
  let triggerElement: HTMLButtonElement | null = null

  const [isOpen, setIsOpen] = useState(false)

  const projects = orderProjectsByRecentActivity(
    projectRegistry.projectList,
    projectContext.recentProjectPaths,
  )
  const activeProject =
    projectContext.activeProjectPath === null
      ? null
      : projectRegistry.projectsByPath[projectContext.activeProjectPath] ?? null

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

  useAppCommand(AppCommand.navigation.openSwitchProject, () => {
    setIsOpen(true)
  })

  useAppCommand(AppCommand.projects.openFolder, async () => {
    await openFolderFromSwitcher()
  })

  <button
    {ref triggerElement}
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

  if (!isOpen) {
    return
  }

  let inputElement: HTMLInputElement | null = null

  const [search, setSearch] = useState("")
  const [highlightedIndex, setHighlightedIndex] = useState(0)
  const [itemElements] = useState((): Record<string, HTMLButtonElement | null> => ({}))

  const items: SwitchProjectItem[] = concat(
    search.trim().length === 0 ? { id: "open-folder", kind: "open-folder" } : undefined,
    projects.filter((project) => projectMatchesSearch(project, search)).map(
      (project) => ({
        id: project.path,
        kind: "project" as const,
        project,
      }),
    ),
  )

  const highlightedItemIndex =
    items.length === 0 ? 0 : Math.min(highlightedIndex, items.length - 1)
  const highlightedItem = items[highlightedItemIndex]
  const highlightedItemId = highlightedItem?.id ?? null

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

  useEffect(() => {
    queueMicrotask(() => {
      if (!inputElement) {
        return
      }

      const length = inputElement.value.length
      inputElement.focus()
      inputElement.setSelectionRange(length, length)
    })
  }, [])

  <div class={styles.content}>
    <label class={styles.searchField}>
      <span class={styles.srOnly}>{"Search projects"}</span>
      <Search aria-hidden={true} size={16} strokeWidth={2.1} />
      <input
        {ref inputElement}
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
        }}
      />
    </label>

    if (items.length > 0) {
      <ul class={styles.list}>
        for (const item of items; index index) {
          <li key={item.id}>
            <button
              {ref (element) => {
                itemElements[item.id] = element
              }}
              class={styles.item}
              data-highlighted={highlightedItemIndex === index ? "true" : "false"}
              type="button"
              onMouseEnter={() => {
                setHighlightedIndex(index)
              }}
              onClick={() => {
                activateItem(item)
              }}
            >
              <span class={styles.itemBody}>
                if (item.kind === "open-folder") {
                  <span class={styles.itemTitle}>{"Open folder…"}</span>
                  <span class={styles.itemDetail}>
                    {"Add a local project from your filesystem."}
                  </span>
                } else {
                  <span class={styles.itemTitleEllipsis}>
                    {item.project.name}
                  </span>
                  <span class={styles.itemDetailEllipsis}>
                    {item.project.path}
                  </span>
                }
              </span>
              if (item.kind === "project" && activeProjectPath?.path === item.project.path) {
                <span class={styles.activeBadge}>{"Active"}</span>
              }
            </button>
          </li>
        }
      </ul>
    } else {
      <p class={styles.empty}>{"No matching projects."}</p>
    }
  </div>
}

export default SwitchProjectDropdown
