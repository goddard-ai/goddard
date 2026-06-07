import { Popover } from "@goddard-ai/ui-primitives"
import { useSignal } from "@preact/signals"
import { useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  recordSessionLaunchAgentUse,
  resolvePreferredLaunchAgentId,
  resolvePreferredLaunchCwd,
} from "~/sessions/launch-preferences.ts"
import { AcpAgentIcon } from "~/sessions/acp-agent-icon.tsrx"
import styles from "./agent-selector.style.ts"

export component AppShellAgentSelector() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null
  const open = useSignal(false)
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const subpackages = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })
  const cwd =
    projectPath ? resolvePreferredLaunchCwd(projectPath, subpackages.subpackages) : null
  const adapterCatalog = useQuery(cwd ? goddardSdk.adapter.list : null, {
    params: cwd ? [{ cwd }] : null,
  })

  if (!adapterCatalog) {
    return
  }

  const agentId = resolvePreferredLaunchAgentId(adapterCatalog)
  const selectedAgent = adapterCatalog.adapters.find((adapter) => adapter.id === agentId) ?? null

  <button
    {ref triggerElementRef}
    aria-expanded={open.value}
    aria-haspopup="menu"
    aria-label={selectedAgent ? `ACP agent: ${selectedAgent.name}` : "Select ACP agent"}
    class={styles.trigger}
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
    modalFocus={false}
    offset={8}
    {open}
    placement="bottom-end"
    role="menu"
    strategy="fixed"
  >
    <ul class={styles.list}>
      for (const adapter of adapterCatalog.adapters) {
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
  </Popover>
}
