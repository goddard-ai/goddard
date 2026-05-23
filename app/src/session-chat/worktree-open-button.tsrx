import { useSignal } from "@preact/signals"
import { AppWindow, ChevronDown, Folder, LoaderCircle, Terminal } from "lucide-react"
import { useRef, useState } from "preact/hooks"

import styles from "./worktree-open-button.style.ts"
import { Menu, MenuItem } from "@goddard-ai/ui-primitives"
import { desktopHost } from "~/desktop-host.ts"
import { queryClient, useQuery } from "~/lib/query.ts"
import type { WorktreeOpener, WorktreeOpenerId } from "~/shared/worktree-openers.ts"

export function WorktreeOpenButton(props: { sessionId: string }) @{
  const openersResponse = useQuery(desktopHost.listWorktreeOpeners, {
    params: [props.sessionId],
    refetchOnWindowReactivate: true,
  })
  const menuOpen = useSignal(false)
  const menuButtonRef = useRef<HTMLButtonElement | null>(null)
  const [pendingOpenerId, setPendingOpenerId] = useState<WorktreeOpenerId | null>(null)
  const [error, setError] = useState<string | null>(null)

  const primaryOpener =
    openersResponse.openers.find((opener) => opener.id === openersResponse.primaryOpenerId) ?? null
  const availableOpeners = openersResponse.openers.filter((opener) => opener.available)
  const isPending = pendingOpenerId !== null
  const disabled = !primaryOpener || isPending

  async function openWith(openerId: WorktreeOpenerId) {
    if (pendingOpenerId !== null) {
      return
    }

    setError(null)
    setPendingOpenerId(openerId)

    try {
      const response = await desktopHost.openWorktree(props.sessionId, openerId)

      if (!response.ok) {
        setError(response.message)
        return
      }

      queryClient.invalidate(desktopHost.listWorktreeOpeners, [props.sessionId])
    } catch (launchError) {
      console.error("Failed to open session worktree.", launchError)
      setError("Couldn’t open this worktree.")
    } finally {
      setPendingOpenerId(null)
    }
  }

  <div class={styles.root}>
    <button
      aria-label={primaryOpener ? `Open in ${primaryOpener.displayName}` : "No opener available"}
      class={styles.primaryButton}
      {disabled}
      type="button"
      onClick={() => {
        if (primaryOpener) {
          void openWith(primaryOpener.id)
        }
      }}
    >
      if (isPending)
      {<LoaderCircle size={15} strokeWidth={2.1} />}
      else
      {<WorktreeOpenerIcon opener={primaryOpener} />}
      <span class={styles.primaryLabel}>
        {primaryOpener ? `Open in ${primaryOpener.displayName}` : "No opener available"}
      </span>
    </button>
    <button
      ref={menuButtonRef}
      aria-expanded={menuOpen.value ? "true" : "false"}
      aria-label="Choose app to open worktree"
      class={styles.menuButton}
      disabled={availableOpeners.length === 0 || isPending}
      type="button"
      onClick={() => {
        menuOpen.value = !menuOpen.value
      }}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") {
          event.preventDefault()
          menuOpen.value = true
        }
      }}
    >
      <ChevronDown size={15} strokeWidth={2.1} />
    </button>
    <Menu
      anchor={() => menuButtonRef.current}
      ariaLabel="Choose app to open worktree"
      class={styles.menu}
      open={menuOpen}
    >
      for (const opener of availableOpeners)
      {<MenuItem
        class={styles.menuItem}
        disabled={pendingOpenerId !== null}
        onSelect={() => {
          void openWith(opener.id)
        }}
      >
        <WorktreeOpenerIcon {opener} />
        <span class={styles.menuItemText}>
          <span class={styles.menuItemName}>{opener.displayName}</span>
          <span class={styles.menuItemMeta}>
            {opener.id === openersResponse.primaryOpenerId ? "Current default" : opener.kind}
          </span>
        </span>
      </MenuItem>}
    </Menu>
    if (error)
    {<span aria-live="polite" class={styles.error} title={error ?? undefined}>{error}</span>}
  </div>
}

function WorktreeOpenerIcon(props: { opener: WorktreeOpener | null }) {
  if (props.opener?.kind === "file-manager") {
    return <Folder size={15} strokeWidth={2.1} />
  }

  if (props.opener?.kind === "terminal") {
    return <Terminal size={15} strokeWidth={2.1} />
  }

  return <AppWindow size={15} strokeWidth={2.1} />
}
