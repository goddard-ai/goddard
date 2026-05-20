import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useComputed, useSignal } from "@preact/signals"
import { useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { Popover } from "~/lib/popover.tsrx"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { openProjectFromFilesystem, openProjectTab } from "./mutations.ts"
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

/** Renders the header-anchored searchable project switcher dropdown. */
export component SwitchProjectDropdown() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const triggerElementRef = useRef<HTMLButtonElement>(null)

  const isOpen = useSignal(false)
  const activeProject =
    projectContext.activeProjectPath === null
      ? null
      : projectRegistry.projectsByPath[projectContext.activeProjectPath] ?? null

  useAppCommand(AppCommand.navigation.openSwitchProject, () => {
    isOpen.value = true
  })

  <button
    {ref triggerElementRef}
    aria-expanded={isOpen.value ? "true" : "false"}
    aria-haspopup="dialog"
    aria-label="Switch project"
    class={styles.trigger}
    type="button"
    onClick={() => {
      isOpen.value = !isOpen.value
    }}
    onKeyDown={(event) => {
      if (event.key === "ArrowDown" && !isOpen.value) {
        event.preventDefault()
        isOpen.value = true
      }
    }}
  >
    <span class={styles.triggerLabel}>
      {activeProject?.name ?? "Open project"}
    </span>
  </button>

  const inputElementRef = useRef<HTMLInputElement>(null)

  <Popover
    anchor={() => triggerElementRef.current}
    ariaLabel="Switch project"
    class={styles.content}
    closeOnEscape={true}
    closeOnOutsidePointer={true}
    initialFocus={() => inputElementRef.current}
    modalFocus={true}
    offset={10}
    open={isOpen}
    placement="bottom-start"
    role="dialog"
    sameWidth={true}
    strategy="fixed"
  >
    const workbenchTabSet = useWorkbenchTabSet()
    const search = useSignal("")
    const highlightedIndex = useSignal(0)

    const projects = useComputed(() => {
      return orderProjectsByRecentActivity(
        projectRegistry.projectList,
        projectContext.recentProjectPaths,
      )
    })

    const searchableProjects = useComputed(() => {
      return projects.value.map(
        (project) => ({
          preparedName: fuzzysort.prepare(project.name),
          preparedPath: fuzzysort.prepare(project.path),
          project,
        }),
      )
    })

    const filteredProjects = useComputed(() => {
      if (isEmptyQuery(search.value)) {
        return projects.value
      }

      return fuzzysort.searchFields(
        search.value,
        searchableProjects.value,
        [
          { key: "name", extract: (item) => item.preparedName },
          { key: "path", extract: (item) => item.preparedPath },
        ],
        { threshold: 0 },
      ).items.map((item) => item.value.project)
    })

    const items = useComputed<SwitchProjectItem[]>(() => {
      const projectItems: SwitchProjectItem[] = filteredProjects.value.map(
        (project) => ({
          id: project.path,
          kind: "project",
          project,
        }),
      )

      if (!isEmptyQuery(search.value)) {
        return projectItems
      }

      const openFolderItem: SwitchProjectItem = { id: "open-folder", kind: "open-folder" }
      return [openFolderItem, ...projectItems]
    })

    const openFolderFromSwitcher = async () => {
      isOpen.value = false
      await openProjectFromFilesystem({
        projectContext,
        projectRegistry,
        workbenchTabSet,
      })
    }

    const highlightedItemIndex =
      items.value.length === 0 ? 0 : Math.min(highlightedIndex.value, items.value.length - 1)
    const highlightedItem = items.value[highlightedItemIndex]

    const moveHighlightedIndex = (delta: -1 | 1) => {
      if (items.value.length === 0) {
        highlightedIndex.value = 0
        return
      }

      const currentHighlightedIndex = Math.min(highlightedIndex.value, items.value.length - 1)

      highlightedIndex.value = delta > 0
        ? Math.min(currentHighlightedIndex + 1, items.value.length - 1)
        : Math.max(currentHighlightedIndex - 1, 0)
    }

    const activateItem = (item: SwitchProjectItem | undefined) => {
      if (!item) {
        return
      }

      if (item.kind === "open-folder") {
        void openFolderFromSwitcher()
        return
      }

      isOpen.value = false
      openProjectTab({
        projectContext,
        projectPath: item.project.path,
        projectRegistry,
        workbenchTabSet,
      })
    }

    <label class={styles.searchField}>
      <span class={styles.srOnly}>"Search projects"</span>
      <Search aria-hidden={true} size={16} strokeWidth={2.1} />
      <input
        {ref inputElementRef}
        class={styles.searchInput}
        placeholder="Search projects"
        value={search.value}
        onInput={(event) => {
          search.value = event.currentTarget.value
          highlightedIndex.value = 0
        }}
        onKeyDown={(event) => {
          switch (event.key) {
            case "ArrowDown":
              event.preventDefault()
              moveHighlightedIndex(1)
              return
            case "ArrowUp":
              event.preventDefault()
              moveHighlightedIndex(-1)
              return
            case "Enter":
              event.preventDefault()
              activateItem(highlightedItem)
          }
        }}
      />
    </label>

    if (items.value.length > 0) {
      <ul class={styles.list}>
        for (const item of items.value; index index) {
          <li key={item.id}>
            <button
              class={styles.item}
              data-highlighted={highlightedItemIndex === index ? "true" : "false"}
              type="button"
              onMouseEnter={() => {
                highlightedIndex.value = index
              }}
              onClick={() => {
                activateItem(item)
              }}
            >
              <span class={styles.itemBody}>
                if (item.kind === "open-folder") {
                  <span class={styles.itemTitle}>"Open folder…"</span>
                  <span class={styles.itemDetail}>"Add a local project from your filesystem."</span>
                } else {
                  <span class={styles.itemTitleEllipsis}>
                    {item.project.name}
                  </span>
                  <span class={styles.itemDetailEllipsis}>
                    {item.project.path}
                  </span>
                }
              </span>
              if (item.kind === "project" && activeProject?.path === item.project.path) {
                <span class={styles.activeBadge}>"Active"</span>
              }
            </button>
          </li>
        }
      </ul>
    } else {
      <p class={styles.empty}>"No matching projects."</p>
    }
  </Popover>
}
