/** Renders the app-wide command palette. */
import { Dialog } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "ark-cmdk"
import { Search } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

const overlayClass = css({
  position: "fixed",
  inset: "0",
  backgroundColor: "overlay",
  backdropFilter: "blur(10px)",
  opacity: "1",
  transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  zIndex: "50",
  "@starting-style": {
    opacity: "0",
  },
})

const contentClass = css({
  width: "min(560px, calc(100vw - 24px))",
  maxHeight: "min(460px, calc(100vh - 32px))",
  padding: "0",
  display: "flex",
  flexDirection: "column",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "20px",
  backgroundColor: "panel",
  boxShadow: `0 24px 56px ${token.var("colors.shadow")}`,
  overflow: "hidden",
  outline: "none",
  opacity: "1",
  transform: "translateY(0) scale(1)",
  transition:
    "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  "@starting-style": {
    opacity: "0",
    transform: "translateY(12px) scale(0.985)",
  },
})

const positionerClass = css({
  position: "fixed",
  inset: "0",
  display: "grid",
  justifyItems: "center",
  alignContent: "start",
  paddingTop: "14vh",
  paddingInline: "12px",
  zIndex: "51",
})

const commandRootClass = css({
  display: "flex",
  flexDirection: "column",
  minHeight: "0",
  width: "100%",
  background: "transparent",
  color: "text",
})

const inputWrapperClass = css({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "56px",
  paddingInline: "16px",
  borderBottom: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusWithin: {
    backgroundColor: "surface",
    borderColor: "accent",
  },
})

const searchIconClass = css({
  flexShrink: "0",
  color: "muted",
})

const inputClass = css({
  width: "100%",
  height: "28px",
  border: "none",
  outline: "none",
  backgroundColor: "transparent",
  color: "text",
  fontSize: "0.94rem",
  fontWeight: "600",
  letterSpacing: "-0.01em",
  "&::placeholder": {
    color: "muted",
  },
})

const listClass = css({
  padding: "8px",
  overflowY: "auto",
  "& [data-scope='combobox'][data-part='item']": {
    outline: "none",
  },
  "& [data-scope='combobox'][data-part='item'] + [data-scope='combobox'][data-part='item']": {
    marginTop: "4px",
  },
  "& [data-scope='combobox'][data-part='item']:focus-visible > div": {
    outline: "2px solid",
    outlineColor: "accentStrong",
    outlineOffset: "2px",
  },
})

const itemClass = css({
  display: "grid",
  gridTemplateColumns: "auto minmax(0, 1fr) auto",
  alignItems: "center",
  gap: "10px",
  width: "100%",
  minHeight: "44px",
  padding: "8px 10px",
  border: "1px solid",
  borderColor: "transparent",
  borderRadius: "14px",
  backgroundColor: "transparent",
  color: "text",
  cursor: "pointer",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  "&[data-active='true']": {
    backgroundColor: "surface",
    borderColor: "accent",
    boxShadow: `0 12px 28px ${token.var("colors.shadow")}`,
  },
})

const emptyClass = css({
  display: "grid",
  placeItems: "center",
  minHeight: "88px",
  color: "muted",
  fontSize: "0.86rem",
  fontWeight: "600",
})

const iconBadgeClass = css({
  display: "grid",
  placeItems: "center",
  width: "30px",
  height: "30px",
  borderRadius: "10px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "accentStrong",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  "&[data-active='true']": {
    borderColor: "accent",
    backgroundColor: "accent",
    color: "accentFg",
  },
})

const labelClass = css({
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: "0.9rem",
  fontWeight: "620",
})

const shortcutClass = css({
  paddingInline: "8px",
  height: "20px",
  borderRadius: "999px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "muted",
  fontSize: "0.68rem",
  fontWeight: "700",
  letterSpacing: "0.04em",
  lineHeight: "20px",
})

const hiddenTitleClass = css({
  position: "absolute",
  width: "1px",
  height: "1px",
  padding: "0",
  margin: "-1px",
  overflow: "hidden",
  clip: "rect(0, 0, 0, 0)",
  whiteSpace: "nowrap",
  border: "0",
})

/** One actionable item shown in the command palette. */
type CommandMenuItem = {
  id: string
  group: string
  icon: LucideIcon
  keywords?: readonly string[]
  label: string
  onSelect: () => void | Promise<void>
  shortcut?: string
}

/** Renders the searchable command palette dialog and its grouped actions. */
export function CommandMenu(props: {
  items: readonly CommandMenuItem[]
  onOpenChange: (open: boolean) => void
  open: boolean
}) {
  const [search, setSearch] = useState("")

  useEffect(() => {
    if (!props.open && search.length > 0) {
      setSearch("")
    }
  }, [props.open, search])

  function handleSelect(item: CommandMenuItem): void {
    props.onOpenChange(false)
    void Promise.resolve(item.onSelect()).catch((error) => {
      console.error("Failed to run command menu action.", error)
    })
  }

  return (
    <Dialog.Root
      open={props.open}
      onOpenChange={(details: { open: boolean }) => {
        props.onOpenChange(details.open)
      }}
    >
      <Portal>
        <Dialog.Backdrop class={overlayClass} />
        <Dialog.Positioner class={positionerClass}>
          <Dialog.Content class={contentClass}>
            <Dialog.Title class={hiddenTitleClass}>Command menu</Dialog.Title>
            <Command.Root<CommandMenuItem>
              class={commandRootClass}
              itemToKeywords={(item) => item.keywords}
              itemToString={(item) => item.label}
              itemToValue={(item) => item.id}
              items={props.items}
              label="Command menu"
              onSearchChange={setSearch}
              onSelect={handleSelect}
              search={search}
            >
              <div class={inputWrapperClass}>
                <Search aria-hidden={true} class={searchIconClass} size={18} />
                <Command.Input
                  autoFocus={true}
                  class={inputClass}
                  placeholder="Type a command or jump to a view"
                />
              </div>
              <Command.List<CommandMenuItem>
                class={listClass}
                empty={<div class={emptyClass}>No matching commands.</div>}
              >
                {(item, state) => {
                  const Icon = item.icon

                  return (
                    <div class={itemClass} data-active={state.active ? "true" : "false"}>
                      <span class={iconBadgeClass} data-active={state.active ? "true" : "false"}>
                        <Icon aria-hidden={true} size={16} strokeWidth={2} />
                      </span>
                      <span class={labelClass}>{item.label}</span>
                      {item.shortcut ? <span class={shortcutClass}>{item.shortcut}</span> : null}
                    </div>
                  )
                }}
              </Command.List>
            </Command.Root>
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  )
}
