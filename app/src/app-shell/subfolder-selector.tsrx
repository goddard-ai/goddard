import { Popover } from "@goddard-ai/ui-primitives"
import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import { Folder } from "lucide-react"
import { useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  recordSessionLaunchCwdUse,
  resolvePreferredLaunchCwd,
} from "~/sessions/launch-preferences.ts"
import styles from "./subfolder-selector.style.ts"

const noDragClass = "electrobun-webkit-app-region-no-drag"

type AppShellSubfolderItem = {
  label: string;
  path: string;
  detail: string;
}

export component AppShellSubfolderSelector() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null
  const open = useSignal(false)
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const response = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })

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
    modalFocus={false}
    offset={8}
    {open}
    placement="bottom-start"
    role="menu"
    strategy="fixed"
  >
    <ul class={styles.list}>
      for (const item of items) {
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
  </Popover>
}
