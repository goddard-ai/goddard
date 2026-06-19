import { t } from "@lingui/core/macro"
import { Popover } from "@goddard-ai/ui-primitives"
import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { Folder } from "lucide-react"
import { useEffect, useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { useQuery } from "~/lib/query.ts"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  recordSessionLaunchCwdUse,
  resolvePreferredLaunchCwd,
} from "~/sessions/launch-preferences.ts"
import styles from "./subfolder-selector.style.ts"

const noDragClass = "electrobun-webkit-app-region-no-drag"
const subfolderSelectorMinimumFuzzyScore = 0.12

type AppShellSubfolderItem = {
  label: string;
  path: string;
  detail: string;
}

type SearchableSubfolderItem = {
  item: AppShellSubfolderItem;
  preparedDetail: ReturnType<typeof fuzzysort.prepare>;
  preparedLabel: ReturnType<typeof fuzzysort.prepare>;
  preparedPath: ReturnType<typeof fuzzysort.prepare>;
}

function normalizeQuery(query: string) {
  return query.trim().toLocaleLowerCase()
}

function hasDirectMatch<T>(query: string, match: fuzzysort.RecordMatch<T>) {
  const normalizedQuery = normalizeQuery(query)

  return normalizedQuery.length > 0 &&
    match.fields.some((field) => field.target.toLocaleLowerCase().includes(normalizedQuery))
}

function isStrongMatch<T>(query: string, match: fuzzysort.RecordMatch<T>) {
  return match.score >= subfolderSelectorMinimumFuzzyScore || hasDirectMatch(query, match)
}

export function AppShellSubfolderSelector(props: {
  shortcutActive?: boolean;
}) @{
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null
  const open = useSignal(false)
  const search = useSignal("")
  const highlightedIndex = useSignal(0)
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const filterInputRef = useRef<HTMLInputElement>(null)
  const itemElementsRef = useRef<Record<string, HTMLButtonElement | null>>({})
  const response = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })

  useEffect(() => {
    if (!open.value) {
      search.value = ""
      highlightedIndex.value = 0
    }
  }, [highlightedIndex, open.value, search])

  if (!projectPath) {
    return null
  }

  const items: AppShellSubfolderItem[] = [
    {
      label: t`Project root`,
      path: projectPath,
      detail: projectPath,
    },
    ...response.subpackages.map(
      (subpackage) => ({
        label: subpackage.relativePath,
        path: subpackage.path,
        detail: subpackage.manifestPath,
      }),
    ),
  ]
  const selectedCwd = resolvePreferredLaunchCwd(projectPath, response.subpackages)
  const selectedItem = items.find((item) => item.path === selectedCwd) ?? items[0]
  const selectedSubfolderLabel =
    selectedCwd === projectPath ? null : selectedItem.label
  const searchableItems: SearchableSubfolderItem[] = items.map(
    (item) => ({
      item,
      preparedDetail: fuzzysort.prepare(item.detail),
      preparedLabel: fuzzysort.prepare(item.label),
      preparedPath: fuzzysort.prepare(item.path),
    }),
  )
  const filteredItems =
    isEmptyQuery(search.value)
      ? items
      : fuzzysort.searchFields(
          search.value,
          searchableItems,
          [
            { key: "label", extract: (item) => item.preparedLabel },
            { key: "path", extract: (item) => item.preparedPath },
            { key: "detail", extract: (item) => item.preparedDetail },
          ],
          { threshold: 0 },
        ).items.filter((item) => isStrongMatch(search.value, item)).map((item) => item.value.item)
  const highlightedItemIndex =
    filteredItems.length === 0 ? 0 : Math.min(highlightedIndex.value, filteredItems.length - 1)
  const highlightedItem = filteredItems[highlightedItemIndex] ?? null

  function moveHighlightedItem(step: 1 | -1) {
    if (filteredItems.length === 0) {
      highlightedIndex.value = 0
      return
    }

    const currentIndex = Math.min(highlightedIndex.value, filteredItems.length - 1)
    highlightedIndex.value = (currentIndex + step + filteredItems.length) % filteredItems.length
  }

  function commitItem(item: AppShellSubfolderItem | null) {
    if (!item) {
      return
    }

    recordSessionLaunchCwdUse(projectPath, item.path)
    open.value = false
  }

  function resetHighlightedItem() {
    const selectedIndex = filteredItems.findIndex((item) => item.path === selectedCwd)
    highlightedIndex.value = !isEmptyQuery(search.value)
      ? 0
      : selectedIndex >= 0
        ? selectedIndex
        : 0
  }

  function openSelectorMenu() {
    resetHighlightedItem()
    open.value = true
  }

  useAppCommand(AppCommand.sessionInput.openSubpackageSelector, openSelectorMenu, {
    active: props.shortcutActive ?? true,
  })

  useEffect(() => {
    if (!open.value || !highlightedItem) {
      return
    }

    itemElementsRef.current[highlightedItem.path]?.scrollIntoView({
      block: "nearest",
    })
  }, [highlightedItem?.path, open.value])

  <>
    <button
      ref={triggerElementRef}
      aria-expanded={open.value}
      aria-haspopup="menu"
      aria-label={selectedSubfolderLabel
        ? t`Working directory: ${selectedSubfolderLabel}`
        : t`Project root`}
      class={cx(styles.trigger, noDragClass)}
      data-icon-only={selectedSubfolderLabel ? undefined : "true"}
      type="button"
      onClick={() => {
        if (open.value) {
          open.value = false
        } else {
          openSelectorMenu()
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" && !open.value) {
          event.preventDefault()
          openSelectorMenu()
        }
      }}
    >
      <Folder aria-hidden={true} class={styles.icon} strokeWidth={2.1} />

      @if (selectedSubfolderLabel) {
        <span class={styles.label}>{selectedSubfolderLabel}</span>
      }
    </button>

    <Popover
      anchor={() => triggerElementRef.current}
      class={styles.menu}
      closeOnEscape={true}
      closeOnOutsidePointer={true}
      initialFocus={() => filterInputRef.current}
      modalFocus={true}
      offset={8}
      {open}
      placement="bottom-start"
      role="menu"
      strategy="fixed"
    >
      <input
        ref={filterInputRef}
        class={styles.filter}
        placeholder={t`Type to filter`}
        value={search.value}
        onInput={(event) => {
          search.value = event.currentTarget.value
          highlightedIndex.value = 0
        }}
        onKeyDown={(event) => {
          switch (event.key) {
            case "ArrowDown":
              event.preventDefault()
              moveHighlightedItem(1)
              return
            case "ArrowUp":
              event.preventDefault()
              moveHighlightedItem(-1)
              return
            case "Enter":
              event.preventDefault()
              commitItem(highlightedItem)
              return
            case "Escape":
              event.preventDefault()
              event.stopPropagation()
              open.value = false
          }
        }}
      />

      @if (filteredItems.length === 0) {
        <div class={styles.empty}>
          {t`No matching directories.`}
        </div>
      } @else {
        <ul class={styles.list}>
          @for (const item of filteredItems) {
            <li key={item.path}>
              <button
                ref={(element) => {
                  itemElementsRef.current[item.path] = element
                }}
                class={styles.item}
                data-active={item.path === selectedCwd ? "true" : undefined}
                data-highlighted={item.path === highlightedItem?.path ? "true" : undefined}
                role="menuitemradio"
                aria-checked={item.path === selectedCwd ? "true" : "false"}
                type="button"
                onMouseEnter={() => {
                  highlightedIndex.value = filteredItems.findIndex(
                    (entry) => entry.path === item.path,
                  )
                }}
                onClick={() => {
                  commitItem(item)
                }}
              >
                <Folder aria-hidden={true} class={styles.icon} strokeWidth={2.1} />

                <span class={styles.itemBody}>
                  <span class={styles.itemLabel}>{item.label}</span>
                  <span class={styles.itemDetail}>{item.detail}</span>
                </span>
              </button>
            </li>
          }
        </ul>
      }
    </Popover>
  </>
}
