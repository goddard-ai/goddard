/** Renders the app-wide command palette powered by cmdk. */
import * as Dialog from "@radix-ui/react-dialog"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "cmdk"
import { Search } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

const overlayClass = css({
  position: "fixed",
  inset: "0",
  background:
    `radial-gradient(circle at top, color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent), transparent 44%), ` +
    "rgba(11, 13, 17, 0.58)",
  backdropFilter: "blur(16px)",
  opacity: "1",
  transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  zIndex: "50",
  "@starting-style": {
    opacity: "0",
  },
})

const contentClass = css({
  position: "fixed",
  top: "20vh",
  left: "50%",
  width: "min(680px, calc(100vw - 32px))",
  maxHeight: "min(520px, calc(100vh - 48px))",
  padding: "0",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "24px",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 32px 96px rgba(6, 10, 18, 0.42)",
  transform: "translateX(-50%)",
  overflow: "hidden",
  outline: "none",
  transition:
    "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  zIndex: "51",
  "@starting-style": {
    opacity: "0",
    transform: "translateX(-50%) translateY(12px) scale(0.985)",
  },
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
  gap: "12px",
  padding: "18px 20px 16px",
  borderBottom: "1px solid",
  borderColor: "border",
})

const searchIconClass = css({
  color: "muted",
})

const inputClass = css({
  width: "100%",
  height: "32px",
  border: "none",
  outline: "none",
  backgroundColor: "transparent",
  color: "text",
  fontSize: "1rem",
  fontWeight: "560",
  letterSpacing: "-0.01em",
  "&::placeholder": {
    color: "muted",
  },
})

const listClass = css({
  maxHeight: "420px",
  overflowY: "auto",
  padding: "10px",
})

const groupClass = css({
  "& [cmdk-group-heading]": {
    paddingInline: "12px",
    paddingTop: "10px",
    paddingBottom: "8px",
    color: token.var("colors.muted"),
    fontSize: "0.72rem",
    fontWeight: "700",
    letterSpacing: "0.12em",
    textTransform: "uppercase",
  },
})

const itemClass = css({
  display: "grid",
  gridTemplateColumns: "auto minmax(0, 1fr) auto",
  alignItems: "center",
  gap: "12px",
  width: "100%",
  minHeight: "54px",
  paddingInline: "12px",
  borderRadius: "16px",
  color: "text",
  cursor: "pointer",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  "&[data-selected='true']": {
    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
    boxShadow: `0 16px 30px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
  },
})

const emptyClass = css({
  display: "grid",
  placeItems: "center",
  minHeight: "96px",
  color: "muted",
  fontSize: "0.92rem",
})

const iconBadgeClass = css({
  display: "grid",
  placeItems: "center",
  width: "34px",
  height: "34px",
  borderRadius: "12px",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  color: "accentStrong",
  boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
})

const labelClass = css({
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: "0.96rem",
  fontWeight: "620",
})

const shortcutClass = css({
  paddingInline: "8px",
  height: "24px",
  borderRadius: "999px",
  backgroundColor: "surface",
  color: "muted",
  fontSize: "0.75rem",
  fontWeight: "700",
  lineHeight: "24px",
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
  const groups = Array.from(new Set(props.items.map((item) => item.group)))

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
    <Dialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay class={overlayClass} />
        <Dialog.Content class={contentClass}>
          <Dialog.Title class={hiddenTitleClass}>Command menu</Dialog.Title>
          <Command
            className={commandRootClass}
            label="Command menu"
            loop={true}
            shouldFilter={true}
          >
            <div class={inputWrapperClass}>
              <Search aria-hidden={true} class={searchIconClass} size={18} />
              <Command.Input
                autoFocus={true}
                className={inputClass}
                placeholder="Type a command or jump to a view"
                value={search}
                onValueChange={setSearch}
              />
            </div>
            <Command.List className={listClass}>
              <Command.Empty className={emptyClass}>No matching commands.</Command.Empty>
              {groups.map((group) => (
                <Command.Group key={group} heading={group} className={groupClass}>
                  {props.items
                    .filter((item) => item.group === group)
                    .map((item) => {
                      const Icon = item.icon

                      return (
                        <Command.Item
                          key={item.id}
                          className={itemClass}
                          keywords={item.keywords ? [...item.keywords] : undefined}
                          value={`${item.label} ${item.keywords?.join(" ") ?? ""}`}
                          onSelect={() => {
                            handleSelect(item)
                          }}
                        >
                          <span class={iconBadgeClass}>
                            <Icon aria-hidden={true} size={17} strokeWidth={2} />
                          </span>
                          <span class={labelClass}>{item.label}</span>
                          {item.shortcut ? (
                            <span class={shortcutClass}>{item.shortcut}</span>
                          ) : null}
                        </Command.Item>
                      )
                    })}
                </Command.Group>
              ))}
            </Command.List>
          </Command>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
