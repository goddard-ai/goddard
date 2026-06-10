import { Popover } from "@goddard-ai/ui-primitives"
import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { useEffect, useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { useQuery } from "~/lib/query.ts"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  recordSessionLaunchAgentUse,
  resolvePreferredLaunchAgentId,
  resolvePreferredLaunchCwd,
} from "~/sessions/launch-preferences.ts"
import { AcpAgentIcon } from "~/sessions/acp-agent-icon.tsrx"
import styles from "./agent-selector.style.ts"

const noDragClass = "electrobun-webkit-app-region-no-drag"
const agentSelectorMinimumFuzzyScore = 0.12

type AgentSelectorItem = {
  adapter: {
    id: string;
    icon?: string | null;
    name: string;
    description?: string | null;
    version: string;
    unofficial?: boolean;
  };
  preparedId: ReturnType<typeof fuzzysort.prepare>;
  preparedName: ReturnType<typeof fuzzysort.prepare>;
  preparedVersion: ReturnType<typeof fuzzysort.prepare>;
  preparedDescription: ReturnType<typeof fuzzysort.prepare>;
  preparedUnofficial: ReturnType<typeof fuzzysort.prepare>;
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
  return match.score >= agentSelectorMinimumFuzzyScore || hasDirectMatch(query, match)
}

export function AppShellAgentSelector(props: {
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
  const subpackages = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })
  const cwd =
    projectPath ? resolvePreferredLaunchCwd(projectPath, subpackages.subpackages) : null
  const adapterCatalog = useQuery(cwd ? goddardSdk.adapter.list : null, {
    params: cwd ? [{ cwd }] : null,
  })

  useEffect(() => {
    if (!open.value) {
      search.value = ""
      highlightedIndex.value = 0
    }
  }, [highlightedIndex, open.value, search])

  if (!adapterCatalog) {
    return null
  }

  const agentId = resolvePreferredLaunchAgentId(adapterCatalog)
  const selectedAgent = adapterCatalog.adapters.find((adapter) => adapter.id === agentId) ?? null
  const searchableAdapters: AgentSelectorItem[] = adapterCatalog.adapters.map(
    (adapter) => ({
      adapter,
      preparedId: fuzzysort.prepare(adapter.id),
      preparedName: fuzzysort.prepare(adapter.name),
      preparedVersion: fuzzysort.prepare(adapter.version),
      preparedDescription: fuzzysort.prepare(adapter.description ?? ""),
      preparedUnofficial: fuzzysort.prepare(adapter.unofficial ? "unofficial" : ""),
    }),
  )
  const filteredAdapters =
    isEmptyQuery(search.value)
      ? adapterCatalog.adapters
      : fuzzysort.searchFields(
          search.value,
          searchableAdapters,
          [
            { key: "id", extract: (item) => item.preparedId },
            { key: "name", extract: (item) => item.preparedName },
            { key: "version", extract: (item) => item.preparedVersion },
            { key: "description", extract: (item) => item.preparedDescription },
            { key: "unofficial", extract: (item) => item.preparedUnofficial },
          ],
          { threshold: 0 },
        ).items.filter((item) => isStrongMatch(search.value, item)).map(
          (item) => item.value.adapter,
        )
  const highlightedAdapterIndex =
    filteredAdapters.length === 0
      ? 0
      : Math.min(highlightedIndex.value, filteredAdapters.length - 1)
  const highlightedAdapter = filteredAdapters[highlightedAdapterIndex] ?? null

  function moveHighlightedAdapter(step: 1 | -1) {
    if (filteredAdapters.length === 0) {
      highlightedIndex.value = 0
      return
    }

    const currentIndex = Math.min(highlightedIndex.value, filteredAdapters.length - 1)
    highlightedIndex.value = (currentIndex + step + filteredAdapters.length) %
    filteredAdapters.length
  }

  function commitAdapter(adapter: (typeof filteredAdapters)[number] | null) {
    if (!adapter) {
      return
    }

    recordSessionLaunchAgentUse(adapter.id)
    open.value = false
  }

  function resetHighlightedAdapter() {
    const selectedIndex = filteredAdapters.findIndex((adapter) => adapter.id === agentId)
    highlightedIndex.value = !isEmptyQuery(search.value)
      ? 0
      : selectedIndex >= 0
        ? selectedIndex
        : 0
  }

  function openSelectorMenu() {
    resetHighlightedAdapter()
    open.value = true
  }

  useAppCommand(AppCommand.sessionInput.openAdapterSelector, openSelectorMenu, {
    active: props.shortcutActive ?? true,
  })

  useEffect(() => {
    if (!open.value || !highlightedAdapter) {
      return
    }

    itemElementsRef.current[highlightedAdapter.id]?.scrollIntoView({
      block: "nearest",
    })
  }, [highlightedAdapter?.id, open.value])

  <>
    <button
      ref={triggerElementRef}
      aria-expanded={open.value}
      aria-haspopup="menu"
      aria-label={selectedAgent ? `Agent harness: ${selectedAgent.name}` : "Select agent harness"}
      class={cx(styles.trigger, noDragClass)}
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
      <AcpAgentIcon class={styles.icon} icon={selectedAgent?.icon} />
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
      placement="bottom-end"
      role="menu"
      strategy="fixed"
    >
      <input
        ref={filterInputRef}
        class={styles.filter}
        placeholder="Type to filter"
        value={search.value}
        onInput={(event) => {
          search.value = event.currentTarget.value
          highlightedIndex.value = 0
        }}
        onKeyDown={(event) => {
          switch (event.key) {
            case "ArrowDown":
              event.preventDefault()
              moveHighlightedAdapter(1)
              return
            case "ArrowUp":
              event.preventDefault()
              moveHighlightedAdapter(-1)
              return
            case "Enter":
              event.preventDefault()
              commitAdapter(highlightedAdapter)
              return
            case "Escape":
              event.preventDefault()
              event.stopPropagation()
              open.value = false
          }
        }}
      />

      @if (filteredAdapters.length === 0) {
        <div class={styles.empty}>No matching agents.</div>
      } @else {
        <ul class={styles.list}>
          @for (const adapter of filteredAdapters) {
            <li key={adapter.id}>
              <button
                ref={(element) => {
                  itemElementsRef.current[adapter.id] = element
                }}
                class={styles.item}
                data-active={adapter.id === agentId ? "true" : undefined}
                data-highlighted={adapter.id === highlightedAdapter?.id ? "true" : undefined}
                role="menuitemradio"
                aria-checked={adapter.id === agentId ? "true" : "false"}
                type="button"
                onMouseEnter={() => {
                  highlightedIndex.value = filteredAdapters.findIndex(
                    (entry) => entry.id === adapter.id,
                  )
                }}
                onClick={() => {
                  commitAdapter(adapter)
                }}
              >
                <AcpAgentIcon class={styles.icon} icon={adapter.icon} />

                <span class={styles.itemBody}>
                  <span class={styles.itemLabel}>{adapter.name}</span>
                  <span class={styles.itemDetail}>
                    {adapter.version}
                    @if (adapter.unofficial) {
                      <>· Unofficial</>
                    }
                  </span>
                </span>
              </button>
            </li>
          }
        </ul>
      }
    </Popover>
  </>
}
