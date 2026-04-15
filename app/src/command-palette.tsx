/** Renders the app-wide command palette. */
import { Dialog, UseDialogReturn } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css } from "@goddard-ai/styled-system/css"
import { Command } from "ark-cmdk"
import { Search } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsx"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandAvailable } from "./commands/command-context.ts"

export default function CommandPalette(props: { dialog: UseDialogReturn }) {
  const { open } = props.dialog
  const shortcutRegistry = useShortcutRegistry()

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!open && search.length > 0) {
      setSearch("")
    }
  }, [open, search])

  const visibleCommands = appCommandList.filter((command) =>
    isCommandAvailable(shortcutRegistry.runtime, command),
  )

  return (
    <Portal>
      <Dialog.Backdrop
        class={css({
          position: "fixed",
          inset: "0",
          backgroundColor: "overlay",
          backdropFilter: "blur(6px)",
          opacity: "1",
          transition: "opacity 160ms cubic-bezier(0.23, 1, 0.32, 1)",
          zIndex: "50",
          "@starting-style": {
            opacity: "0",
          },
        })}
      />
      <Dialog.Positioner
        class={css({
          position: "fixed",
          inset: "0",
          display: "grid",
          justifyItems: "center",
          alignContent: "start",
          padding: "56px 12px 12px",
          zIndex: "51",
        })}
      >
        <Dialog.Content
          class={css({
            width: "min(560px, calc(100vw - 24px))",
            maxHeight: "min(480px, calc(100vh - 68px))",
            padding: "0",
            display: "flex",
            flexDirection: "column",
            minHeight: "0",
            border: "1px solid",
            borderColor: "border",
            borderRadius: "16px",
            backgroundColor: "panel",
            overflow: "hidden",
            outline: "none",
            opacity: "1",
            transform: "translateY(0)",
            transition:
              "opacity 160ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
            "@starting-style": {
              opacity: "0",
              transform: "translateY(8px)",
            },
          })}
        >
          <Dialog.Title
            class={css({
              position: "absolute",
              width: "1px",
              height: "1px",
              padding: "0",
              margin: "-1px",
              overflow: "hidden",
              clip: "rect(0, 0, 0, 0)",
              whiteSpace: "nowrap",
              border: "0",
            })}
          >
            Command menu
          </Dialog.Title>
          <Command.Root<AppCommand>
            class={css({
              display: "flex",
              flexDirection: "column",
              minHeight: "0",
              width: "100%",
              background: "transparent",
              color: "text",
            })}
            itemToKeywords={(command) => command.keywords}
            itemToString={(command) => command.label}
            itemToValue={(command) => command.id}
            items={visibleCommands}
            label="Command menu"
            onSearchChange={setSearch}
            onSelect={(command) => {
              props.dialog.setOpen(false)
              command()
            }}
            search={search}
          >
            <div
              class={css({
                display: "flex",
                alignItems: "center",
                gap: "10px",
                minHeight: "48px",
                paddingInline: "14px",
                borderBottom: "1px solid",
                borderColor: "border",
              })}
            >
              <Search
                aria-hidden={true}
                class={css({
                  flexShrink: "0",
                  color: "muted",
                })}
                size={16}
              />
              <Command.Input
                autoFocus={true}
                class={css({
                  width: "100%",
                  height: "24px",
                  border: "none",
                  outline: "none",
                  backgroundColor: "transparent",
                  color: "text",
                  fontSize: "0.9rem",
                  "&::placeholder": {
                    color: "muted",
                  },
                })}
                placeholder="Type a command"
              />
            </div>
            <Command.List<AppCommand>
              class={css({
                padding: "6px",
                overflowY: "auto",
                "& [data-scope='combobox'][data-part='item']": {
                  outline: "none",
                },
                "& [data-scope='combobox'][data-part='item']:focus-visible": {
                  outline: "2px solid",
                  outlineColor: "accentStrong",
                  outlineOffset: "-2px",
                  borderRadius: "10px",
                },
              })}
              empty={
                <div
                  class={css({
                    padding: "20px 14px",
                    color: "muted",
                    fontSize: "0.88rem",
                    textAlign: "center",
                  })}
                >
                  No matching commands.
                </div>
              }
            >
              {(item, state) => {
                const Icon = item.icon
                const shortcutBinding = shortcutRegistry.resolvedBindings[item.id]?.[0]
                const shortcut =
                  typeof shortcutBinding === "string"
                    ? shortcutBinding
                    : (shortcutBinding?.combo ?? shortcutBinding?.sequence)

                return (
                  <div
                    class={css({
                      display: "grid",
                      gridTemplateColumns: Icon
                        ? "16px minmax(0, 1fr) auto"
                        : "minmax(0, 1fr) auto",
                      alignItems: "center",
                      gap: "10px",
                      width: "100%",
                      minHeight: "40px",
                      padding: "8px 10px",
                      borderRadius: "10px",
                      backgroundColor: "transparent",
                      color: "text",
                      cursor: "pointer",
                      "&[data-active='true']": {
                        backgroundColor: "surface",
                      },
                    })}
                    data-active={state.active ? "true" : "false"}
                  >
                    {Icon ? (
                      <Icon
                        aria-hidden={true}
                        className={css({
                          color: state.active ? "text" : "muted",
                        })}
                        size={16}
                        strokeWidth={2}
                      />
                    ) : null}
                    <span
                      class={css({
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontSize: "0.9rem",
                        fontWeight: "600",
                      })}
                    >
                      {item.label}
                    </span>
                    {shortcut ? (
                      <span
                        class={css({
                          paddingInline: "6px",
                          minWidth: "0",
                          height: "20px",
                          borderRadius: "6px",
                          border: "1px solid",
                          borderColor: "border",
                          backgroundColor: "background",
                          color: "muted",
                          fontSize: "0.7rem",
                          lineHeight: "18px",
                        })}
                      >
                        {shortcut}
                      </span>
                    ) : null}
                  </div>
                )
              }}
            </Command.List>
          </Command.Root>
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  )
}
