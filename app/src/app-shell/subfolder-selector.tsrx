import { Popover } from "@goddard-ai/ui-primitives"
import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { Folder } from "lucide-react"
import { useEffect, useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
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

export component AppShellSubfolderSelector() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null
  const open = useSignal(false)
  const search = useSignal("")
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const filterInputRef = useRef<HTMLInputElement>(null)
  const response = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })

  useEffect(() => {
    if (!open.value) {
      search.value = ""
    }
  }, [open.value, search])

  if (!projectPath) {
    return
  }

  const items: AppShellSubfolderItem[] = [
    {
      label: "Project root",
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

  <button
    {ref triggerElementRef}
    aria-expanded={open.value}
    aria-haspopup="menu"
    aria-label={selectedSubfolderLabel
      ? `Working directory: ${selectedSubfolderLabel}`
      : "Project root"}
    class={cx(styles.trigger, noDragClass)}
    data-icon-only={selectedSubfolderLabel ? undefined : "true"}
    type="button"
    onClick={() => {
      open.value = !open.value
    }}
    onKeyDown={(event) => {
      if (event.key === "ArrowDown" && !open.value) {
        event.preventDefault()
        open.value = true
      }
    }}
  >
    <Folder aria-hidden={true} class={styles.icon} strokeWidth={2.1} />

    if (selectedSubfolderLabel) {
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
      {ref filterInputRef}
      class={styles.filter}
      placeholder="Type to filter"
      value={search.value}
      onInput={(event) => {
        search.value = event.currentTarget.value
      }}
    />

    if (filteredItems.length === 0) {
      <div class={styles.empty}>"No matching directories."</div>
    } else {
      <ul class={styles.list}>
        for (const item of filteredItems) {
          <li key={item.path}>
            <button
              class={styles.item}
              data-active={item.path === selectedCwd ? "true" : undefined}
              role="menuitemradio"
              aria-checked={item.path === selectedCwd ? "true" : "false"}
              type="button"
              onClick={() => {
                recordSessionLaunchCwdUse(projectPath, item.path)
                open.value = false
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
}
