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

export component AppShellAgentSelector() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null
  const open = useSignal(false)
  const search = useSignal("")
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const filterInputRef = useRef<HTMLInputElement>(null)
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
    }
  }, [open.value, search])

  useAppCommand(AppCommand.sessionInput.openAdapterSelector, () => {
    open.value = true
  }, { active: adapterCatalog != null })

  if (!adapterCatalog) {
    return
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

  <button
    {ref triggerElementRef}
    aria-expanded={open.value}
    aria-haspopup="menu"
    aria-label={selectedAgent ? `ACP agent: ${selectedAgent.name}` : "Select ACP agent"}
    class={cx(styles.trigger, noDragClass)}
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
      {ref filterInputRef}
      class={styles.filter}
      placeholder="Type to filter"
      value={search.value}
      onInput={(event) => {
        search.value = event.currentTarget.value
      }}
    />

    if (filteredAdapters.length === 0) {
      <div class={styles.empty}>"No matching agents."</div>
    } else {
      <ul class={styles.list}>
        for (const adapter of filteredAdapters) {
          <li key={adapter.id}>
            <button
              class={styles.item}
              data-active={adapter.id === agentId ? "true" : undefined}
              role="menuitemradio"
              aria-checked={adapter.id === agentId ? "true" : "false"}
              type="button"
              onClick={() => {
                recordSessionLaunchAgentUse(adapter.id)
                open.value = false
              }}
            >
              <AcpAgentIcon class={styles.icon} icon={adapter.icon} />

              <span class={styles.itemBody}>
                <span class={styles.itemLabel}>{adapter.name}</span>
                <span class={styles.itemDetail}>
                  {adapter.version}
                  if (adapter.unofficial) {
                    " · Unofficial"
                  }
                </span>
              </span>
            </button>
          </li>
        }
      </ul>
    }
  </Popover>
}
