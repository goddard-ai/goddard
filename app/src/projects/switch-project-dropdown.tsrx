import { cx } from "@goddard-ai/styled-system/css"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useComputed, useSignal } from "@preact/signals"
import { useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { Popover, useSearchNavigation } from "@goddard-ai/ui-primitives"
import { text } from "~/language/text.ts"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { openProjectFromFilesystem, openProjectTab } from "./mutations.ts"
import { orderProjectsByRecentActivity } from "./project-context.ts"
import type { ProjectRecord } from "./project-registry.ts"
import styles from "./switch-project-dropdown.style.ts"

const noDragClass = "electrobun-webkit-app-region-no-drag"

type SwitchProjectItem =
  | {
      id: "open-folder";
      kind: "open-folder";
    }
  | {
      id: string;
      kind: "project";
      project: ProjectRecord;
    }

/** Renders the header-anchored searchable project switcher dropdown. */
export function SwitchProjectDropdown() @{
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const inputElementRef = useRef<HTMLInputElement>(null)

  const isOpen = useSignal(false)
  const workbenchTabSet = useWorkbenchTabSet()
  const search = useSignal("")
  const activeProject =
    projectContext.activeProjectPath === null
      ? null
      : projectRegistry.projectsByPath[projectContext.activeProjectPath] ?? null
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

  useAppCommand(AppCommand.navigation.openSwitchProject, () => {
    isOpen.value = true
  })

  const openFolderFromSwitcher = async () => {
    isOpen.value = false
    await openProjectFromFilesystem({
      projectContext,
      projectRegistry,
      workbenchTabSet,
    })
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

  const navigation = useSearchNavigation({
    count: () => items.value.length,
    wrap: false,
    onActivate(index) {
      activateItem(items.value[index])
    },
    onQueryChange(query) {
      search.value = query
    },
  })

  <>
    <button
      ref={triggerElementRef}
      aria-expanded={isOpen.value ? "true" : "false"}
      aria-haspopup="dialog"
      aria-label={text.switchProject}
      class={cx(styles.trigger, noDragClass)}
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
        {activeProject?.name ?? text.openProject}
      </span>
    </button>

    <Popover
      anchor={() => triggerElementRef.current}
      ariaLabel={text.switchProject}
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
      <label class={styles.searchField}>
        <span class={styles.srOnly}>{text.searchProjects}</span>
        <Search aria-hidden={true} size={16} strokeWidth={2.1} />
        <input
          ref={(element) => {
            inputElementRef.current = element
            const cleanup = navigation.inputRef(element)

            return () => {
              cleanup?.()

              if (inputElementRef.current === element) {
                inputElementRef.current = null
              }
            }
          }}
          class={styles.searchInput}
          placeholder={text.searchProjects}
          value={search.value}
        />
      </label>

      @if (items.value.length > 0) {
        <ul class={styles.list}>
          @for (const item of items.value; index index) {
            <li key={item.id}>
              <button
                ref={navigation.itemRef(index)}
                class={styles.item}
                type="button"
                onClick={() => {
                  activateItem(item)
                }}
              >
                <span class={styles.itemBody}>
                  @if (item.kind === "open-folder") {
                    <>
                      <span class={styles.itemTitle}>{text.openFolderEllipsis}</span>
                      <span
                        class={styles.itemDetail}
                      >{text.addALocalProjectFromYourFilesystem}</span>
                    </>
                  } @else {
                    <>
                      <span class={styles.itemTitleEllipsis}>{item.project.name}</span>
                      <span class={styles.itemDetailEllipsis}>{item.project.path}</span>
                    </>
                  }
                </span>

                @if (item.kind === "project" && activeProject?.path === item.project.path) {
                  <span class={styles.activeBadge}>{text.active}</span>
                }
              </button>
            </li>
          }
        </ul>
      } @else {
        <p class={styles.empty}>{text.noMatchingProjects}</p>
      }
    </Popover>
  </>
}
